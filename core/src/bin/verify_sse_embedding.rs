use safetensors::{tensor::TensorView, Dtype, SafeTensors};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;

#[derive(Debug)]
struct SseModel {
    tokenizer: Tokenizer,
    hidden_dim: usize,
    vocab_size: usize,
    packed: Vec<u8>,
    scales: Vec<f32>,
    alpha: Vec<f32>,
    beta: Vec<f32>,
    bias: Vec<f32>,
}

#[derive(Debug)]
struct Cli {
    model_dir: PathBuf,
    top_k: usize,
    queries: Vec<String>,
    candidates: Vec<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = parse_cli()?;
    let model = SseModel::load(&cli.model_dir)?;

    let queries = if cli.queries.is_empty() {
        vec![
            "星を見に行きたい".to_string(),
            "雨の日の散歩が好き".to_string(),
            "カレーを作るのが楽しい".to_string(),
            "最近ぜんぜん眠れない".to_string(),
        ]
    } else {
        cli.queries
    };

    let candidates = if cli.candidates.is_empty() {
        vec![
            "宇宙".to_string(),
            "天体観測".to_string(),
            "星空".to_string(),
            "天気".to_string(),
            "雨".to_string(),
            "散歩".to_string(),
            "料理".to_string(),
            "カレー".to_string(),
            "睡眠".to_string(),
            "不眠".to_string(),
            "音楽".to_string(),
            "ゲーム".to_string(),
        ]
    } else {
        cli.candidates
    };

    let candidate_embs = candidates
        .iter()
        .map(|text| model.encode(text))
        .collect::<Result<Vec<_>, _>>()?;

    println!(
        "MODEL_OK hidden_dim={} vocab_size={} candidates={} queries={}",
        model.hidden_dim,
        model.vocab_size,
        candidates.len(),
        queries.len()
    );

    for query in &queries {
        let query_emb = model.encode(query)?;
        let mut semantic = Vec::<(String, f32)>::new();
        let mut lexical = Vec::<(String, f32)>::new();

        for (idx, candidate) in candidates.iter().enumerate() {
            let score = cosine_similarity(&query_emb, &candidate_embs[idx]);
            semantic.push((candidate.clone(), score));

            let l = lexical_score(query, candidate);
            lexical.push((candidate.clone(), l));
        }

        sort_desc(&mut semantic);
        sort_desc(&mut lexical);

        println!("\nQUERY: {query}");
        println!("  semantic top-{}", cli.top_k);
        for (name, score) in semantic.iter().take(cli.top_k) {
            println!("    - {name}\tscore={score:.4}");
        }
        println!("  lexical top-{}", cli.top_k);
        for (name, score) in lexical.iter().take(cli.top_k) {
            println!("    - {name}\tscore={score:.4}");
        }
    }

    Ok(())
}

impl SseModel {
    fn load(model_dir: &Path) -> Result<Self, String> {
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|err| format!("tokenizer load: {err}"))?;

        let rest_path = model_dir.join("model_rest.safetensors");
        let rest_bytes = fs::read(rest_path).map_err(|err| format!("read model_rest: {err}"))?;
        let safetensors = SafeTensors::deserialize(&rest_bytes)
            .map_err(|err| format!("parse model_rest.safetensors: {err}"))?;

        let alpha = read_f32_tensor(&safetensors, "dyt.alpha")?;
        let beta = read_f32_tensor(&safetensors, "dyt.beta")?;
        let bias = read_f32_tensor(&safetensors, "dyt.bias")?;

        if alpha.len() != beta.len() || alpha.len() != bias.len() {
            return Err("invalid dyt tensor sizes".to_string());
        }
        let hidden_dim = alpha.len();
        if hidden_dim == 0 || hidden_dim % 2 != 0 {
            return Err(format!(
                "hidden dimension must be positive and even, got {}",
                hidden_dim
            ));
        }

        let emb_path = model_dir.join("embedding.q4_k_m.bin");
        let emb_bytes = fs::read(emb_path).map_err(|err| format!("read embedding: {err}"))?;

        let bytes_per_row = hidden_dim / 2 + 4;
        if emb_bytes.is_empty() || emb_bytes.len() % bytes_per_row != 0 {
            return Err(format!(
                "invalid embedding binary size: {} bytes (bytes_per_row={bytes_per_row})",
                emb_bytes.len()
            ));
        }
        let vocab_size = emb_bytes.len() / bytes_per_row;
        let packed_size = vocab_size * hidden_dim / 2;

        let packed = emb_bytes[..packed_size].to_vec();
        let scale_bytes = &emb_bytes[packed_size..];
        let scales = scale_bytes
            .chunks_exact(4)
            .map(|chunk| {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(chunk);
                f32::from_le_bytes(bytes)
            })
            .collect::<Vec<_>>();

        if scales.len() != vocab_size {
            return Err(format!(
                "invalid scale length: expected {}, got {}",
                vocab_size,
                scales.len()
            ));
        }

        Ok(Self {
            tokenizer,
            hidden_dim,
            vocab_size,
            packed,
            scales,
            alpha,
            beta,
            bias,
        })
    }

    fn encode(&self, text: &str) -> Result<Vec<f32>, String> {
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|err| format!("tokenize failed for '{text}': {err}"))?;
        let ids = encoding.get_ids();

        if ids.is_empty() {
            return Ok(vec![0.0; self.hidden_dim]);
        }

        let mut acc = vec![0.0_f32; self.hidden_dim];
        for id in ids {
            self.add_dequantized_row(*id as usize, &mut acc)?;
        }
        let denom = ids.len() as f32;
        for value in &mut acc {
            *value /= denom;
        }

        for i in 0..self.hidden_dim {
            let x = self.alpha[i] * acc[i] + self.bias[i];
            acc[i] = self.beta[i] * x.tanh();
        }

        l2_normalize(&mut acc);
        Ok(acc)
    }

    fn add_dequantized_row(&self, token_id: usize, out: &mut [f32]) -> Result<(), String> {
        if token_id >= self.vocab_size {
            return Err(format!(
                "token id out of range: id={} vocab_size={}",
                token_id, self.vocab_size
            ));
        }
        let row_scale = self.scales[token_id];
        let row_start = token_id * (self.hidden_dim / 2);
        for pair_idx in 0..(self.hidden_dim / 2) {
            let byte = self.packed[row_start + pair_idx];
            let hi = ((byte >> 4) & 0x0F) as f32;
            let lo = (byte & 0x0F) as f32;

            let dim_hi = pair_idx * 2;
            let dim_lo = dim_hi + 1;

            out[dim_hi] += ((hi / 7.5) - 1.0) * row_scale;
            out[dim_lo] += ((lo / 7.5) - 1.0) * row_scale;
        }
        Ok(())
    }
}

fn read_f32_tensor(safetensors: &SafeTensors<'_>, name: &str) -> Result<Vec<f32>, String> {
    let tensor = safetensors
        .tensor(name)
        .map_err(|err| format!("missing tensor {name}: {err}"))?;
    tensor_view_to_f32_vec(&tensor)
}

fn tensor_view_to_f32_vec(view: &TensorView<'_>) -> Result<Vec<f32>, String> {
    if view.dtype() != Dtype::F32 {
        return Err(format!("expected f32 tensor, got {:?}", view.dtype()));
    }
    let bytes = view.data();
    if bytes.len() % 4 != 0 {
        return Err(format!("invalid f32 tensor byte size: {}", bytes.len()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| {
            let mut raw = [0u8; 4];
            raw.copy_from_slice(chunk);
            f32::from_le_bytes(raw)
        })
        .collect())
}

fn l2_normalize(values: &mut [f32]) {
    let norm_sq = values.iter().map(|value| value * value).sum::<f32>();
    if norm_sq <= 0.0 {
        return;
    }
    let norm = norm_sq.sqrt();
    for value in values {
        *value /= norm;
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn lexical_score(query: &str, candidate: &str) -> f32 {
    let terms = extract_query_terms_v0(query, 12);
    if terms.is_empty() {
        return 0.0;
    }
    let c = candidate.to_lowercase();
    let hit = terms
        .iter()
        .filter(|term| c.contains(&term.to_lowercase()))
        .count();
    if hit == 0 {
        0.0
    } else {
        hit as f32 / terms.len() as f32
    }
}

fn extract_query_terms_v0(input_text: &str, max_terms: usize) -> Vec<String> {
    let normalized = normalize_input_text_v0(input_text);
    if normalized.is_empty() {
        return Vec::new();
    }
    let segments = segment_by_char_class(&normalized);
    let mut terms = Vec::<String>::new();
    for segment in segments {
        append_segment_variants(&mut terms, &segment);
    }
    dedupe_and_filter_terms(terms, max_terms.max(1))
}

fn normalize_input_text_v0(input_text: &str) -> String {
    let mut chars = String::with_capacity(input_text.len());
    for ch in input_text.chars() {
        let normalized = normalize_full_width_char(ch);
        if normalized.is_whitespace() || is_punctuation_like(normalized) {
            chars.push(' ');
        } else {
            chars.push(normalized);
        }
    }
    chars.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_full_width_char(ch: char) -> char {
    match ch {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    }
}

fn is_punctuation_like(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '。' | '、'
                | '・'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '【'
                | '】'
                | '［'
                | '］'
                | '｛'
                | '｝'
                | '〈'
                | '〉'
                | '《'
                | '》'
                | '！'
                | '？'
                | '：'
                | '；'
                | '，'
                | '．'
                | '〜'
                | '～'
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Kanji,
    Hiragana,
    Katakana,
    Alnum,
    Other,
}

fn classify_char(ch: char) -> CharClass {
    if ch.is_ascii_alphanumeric() || ('０'..='９').contains(&ch) || ('ａ'..='ｚ').contains(&ch)
    {
        return CharClass::Alnum;
    }
    if ('\u{4E00}'..='\u{9FFF}').contains(&ch) || ('\u{3400}'..='\u{4DBF}').contains(&ch) {
        return CharClass::Kanji;
    }
    if ('\u{3040}'..='\u{309F}').contains(&ch) {
        return CharClass::Hiragana;
    }
    if ('\u{30A0}'..='\u{30FF}').contains(&ch) || ('\u{31F0}'..='\u{31FF}').contains(&ch) {
        return CharClass::Katakana;
    }
    CharClass::Other
}

fn segment_by_char_class(input: &str) -> Vec<String> {
    let mut segments = Vec::<String>::new();
    let mut current = String::new();
    let mut current_class = CharClass::Other;
    for ch in input.chars() {
        let class = classify_char(ch);
        if class == CharClass::Other {
            if !current.is_empty() {
                segments.push(current.clone());
                current.clear();
            }
            current_class = CharClass::Other;
            continue;
        }
        if current.is_empty() || class == current_class {
            current.push(ch);
            current_class = class;
            continue;
        }
        segments.push(current.clone());
        current.clear();
        current.push(ch);
        current_class = class;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn append_segment_variants(out: &mut Vec<String>, segment: &str) {
    let chars = segment.chars().collect::<Vec<_>>();
    let len = chars.len();
    if len == 0 {
        return;
    }
    out.push(segment.to_string());
    if len >= 3 {
        out.push(chars[..(len - 1)].iter().collect::<String>());
        out.push(chars[1..].iter().collect::<String>());
    }
    if len >= 4 {
        out.push(chars[1..(len - 1)].iter().collect::<String>());
    }
}

fn dedupe_and_filter_terms(terms: Vec<String>, max_terms: usize) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for term in terms {
        let cleaned = term.trim();
        if !is_valid_query_term(cleaned) {
            continue;
        }
        let key = cleaned.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(cleaned.to_string());
        if out.len() >= max_terms {
            break;
        }
    }
    out
}

fn is_valid_query_term(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.chars().count() == 1 {
        return value
            .chars()
            .next()
            .map(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
            .unwrap_or(false);
    }
    value
        .chars()
        .all(|ch| classify_char(ch) != CharClass::Other && !ch.is_whitespace())
}

fn sort_desc(items: &mut [(String, f32)]) {
    items.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
}

fn parse_cli() -> Result<Cli, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }

    let mut model_dir = PathBuf::from("/tmp/sse-ja");
    let mut top_k = 5usize;
    let mut queries = Vec::<String>::new();
    let mut candidates = Vec::<String>::new();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--model-dir" => {
                let value = args.get(i + 1).ok_or("--model-dir requires a value")?;
                model_dir = PathBuf::from(value);
                i += 2;
            }
            "--top-k" => {
                let value = args.get(i + 1).ok_or("--top-k requires a value")?;
                top_k = value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid --top-k: {err}"))?
                    .max(1);
                i += 2;
            }
            "--query" => {
                let value = args.get(i + 1).ok_or("--query requires a value")?;
                queries.push(value.clone());
                i += 2;
            }
            "--candidate" => {
                let value = args.get(i + 1).ok_or("--candidate requires a value")?;
                candidates.push(value.clone());
                i += 2;
            }
            value => {
                return Err(format!("unknown option: {value}"));
            }
        }
    }

    if !model_dir.exists() {
        return Err(format!(
            "model directory not found: {}",
            model_dir.display()
        ));
    }

    Ok(Cli {
        model_dir,
        top_k,
        queries,
        candidates,
    })
}

fn print_usage() {
    println!("verify_sse_embedding");
    println!("Usage:");
    println!("  cargo run --bin verify_sse_embedding -- [options]");
    println!();
    println!("Options:");
    println!("  --model-dir <PATH>    Model directory (default: /tmp/sse-ja)");
    println!("  --top-k <N>           Number of top results (default: 5)");
    println!("  --query <TEXT>        Query text (repeatable)");
    println!("  --candidate <TEXT>    Candidate concept text (repeatable)");
    println!();
    println!("If query/candidate are omitted, built-in Japanese demo cases are used.");
}
