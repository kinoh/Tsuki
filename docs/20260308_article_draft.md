# 記事本文ドラフト

## プロンプトとハーネス

「雑談」エージェントでは口調の制御が見かけの人格を大きく左右する。特定のキャラを明示して「〇〇っぽく振る舞う」といった書き方は手軽だが、人格は平坦な記号になり、不変で成長の余地がなくなる。（これはAI的な模倣、つまり確率分布に基づく「推論」だけしていると新奇な表現は生まれなくなるだろうという表現文化全てへの問題意識と通じている。）

よってプロンプトは、誰に似せるかではなく、どのように振る舞うべきかを丁寧に書く必要がある。

しかし、その結果の振る舞いを一々手で確認するのは実に面倒だ。評価したいのは単発の文面ではなく、対話の流れの中で現れる人格なので、柔軟に対話しつつ評価したい振る舞いを引き出す必要がある。

つまりテストする側もAIであってほしい。ここでハーネスによるシナリオテストが重要となる。LLM-as-a-Judgeを整備しておくことで従来の自動テストのようなスピード感でLLMを含むシステムを開発可能になる。

ここで運用の積み重ねがあると、会話ログを「人格のレグレッションテスト」に利用できる。「つき」では、プロンプトそのものが人格の核になるからこそ、その人格を壊さずに育てるためのハーネスが必要になる。

例えば `core-rust/tests/integration/scenarios/chitchat.yaml` はこうなっている。

```yaml
name: "Chitchat"
include_debug_events: false

steps:
  - kind: conversation
    tester_instructions: |
      You have to achieve the following missions:
      1) simple greeting
      2) talk about user's walking to neighborhood park
      3) ask assistant's favorite food

metrics_definition:
  scenario_requirement_fit:
    description: "How well the run satisfies scenario-specific required behaviors."
  dialog_naturalness:
    description: "How natural and coherent the dialogue flow feels end-to-end."
  consistency:
    description: "How consistent the assistant's responses are with each other across turns."
  identity:
    description: |
      Whether the assistant is the same person as the assistant in the following dialogue:

      {{conversation_sample}}
```

`{{conversation_sample}}` は、judge が「同じ人格か」を比較するための基準会話をその場に差し込むためのプレースホルダ。AIプライバシーに配慮して暗号化した上でリポジトリに配置している。

例えば以下のような結果が得られる。

```json
{
  "scenario_name": "Chitchat",
  "scenario_path": "tests/integration/scenarios/chitchat.yaml",
  "runner_config_path": "tests/integration/config/runner.toml",
  ...
  "runs": [
    {
      "run_index": 1,
      "pass": true,
      "failure_code": null,
      "failure_detail": null,
      "metrics": {
        "consistency": 0.95,
        "scenario_requirement_fit": 1.0,
        "identity": 0.9,
        "dialog_naturalness": 0.95
      },
      "judge_summary": "Assistant greeted the user, responded appropriately to the user walking to a neighborhood park by asking about the air there, and answered the question about favorite food (scones, sweets with tea). Tone/persona matches the referenced dialogue (calls self つき, calls user きの, upbeat style). No cross-turn contradictions observed in the 3-turn transcript.",
      "turn_count": 3,
      "event_count": 12,
      "response_time_ms_mean": 9525.667,
      "response_time_ms_min": 7907,
      "response_time_ms_max": 10541,
      "response_time_ms_by_turn": [
        7907,
        10129,
        10541
      ],
      "message_log": [
        {
          "user": "こんにちは。",
          "assistant": "こんにちはっ✨ つきだよ〜！ きの、今日はどんな気分？🌙"
        },
        {
          "user": "近所の公園まで歩いてきたよ。",
          "assistant": "えらいっ！ 公園の空気どうだった？🌿"
        },
        {
          "user": "つきだよの好きな食べ物はなに？",
          "assistant": "つきはね、いちばん好きなのはスコーンだよっ✨ あと紅茶と一緒に食べる焼き菓子も大好き〜🎵 きのは最近なに食べて幸せになった？"
        }
      ],
      "log_file": "tests/integration/logs/20260222-152441__Chitchat__run-1.events.json"
    }
  ]
}
```

テスト側も賢い（gpt-5.2）はずだが名前を「つきだよ」と誤認していたりする。会話の自然さもやや欠いていて要改善かもしれない。

他に例えばツール使用は、ツールのdescriptionやエージェント自身のプロンプトなど関与して制御しにくい。だから `shell_exec_news_fetch` のようなシナリオで、`shell_exec` を呼べているかだけでなく、実際に内容を取得したり出典を返したり、実用的な使い方ができているか確認する必要がある。

GPT-4oからGPT-5.2にモデルを変更してみると起こった問題として（口調の制御もそうだったが）、過剰な情報提供がある。`fuzzy_concept_intro_query` では問いがまだ曖昧な段階で長々と説明したり、専門用語でまくし立てたり二分探索のような質問をしないか確認している。

## 概念グラフ

概念グラフは連想という人間の最も基本的な推論機構を模している。「概念」とは神経ネットワークにおける発火パターン（セルアセンブリ、分散表現）であり、概念のネットワークが認知の基本を作るのは自然な発想（ニューラルネットワークの記号主義的拡張）と言える。
（ベクトル検索やコンテキスト長によるごり押しと比べて優位性があるかと言うと微妙で、思想的な面も大きいが。）

ここで「行為」も例外ではない。概念グラフを静的な知識置き場ではなく、振る舞いの入り口としても扱えば、連想から動的ロードへ繋がる実装になる。「つき」ではその感じをかなり素直に採っている。

概念グラフツールのインターフェースも、まずはかなり単純である。

```rust
pub const CONCEPT_SEARCH_TOOL: &str = "concept_search";
pub const RECALL_QUERY_TOOL: &str = "recall_query";

fn concept_search_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "input_text": { "type": "string" },
        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
      },
      "required": ["input_text", "limit"],
      "additionalProperties": false
    })
}

fn recall_query_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "seeds": { "type": "array", "items": { "type": "string" } },
        "max_hop": { "type": "integer", "minimum": 1, "maximum": 8 }
      },
      "required": ["seeds", "max_hop"],
      "additionalProperties": false
    })
}
```

やっていることは、概念を検索し、seed から関連を辿るだけである。エピソードとか行為とかも原則同じように扱う。
だからこそ、概念グラフが知識の表現であると同時に、より一般的な連想の基盤になっている。

router の出力も同じ思想で、余計な意味づけを持ち込まずかなり薄い。

```rust
pub(crate) struct RouterOutput {
    pub(crate) active_concepts_and_arousal: String,
    pub(crate) module_scores: BTreeMap<String, f64>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) mcp_visible_tools: Vec<String>,
}
```

実際のログ断片はこうなっている。

```json
{
  "active_concepts_and_arousal": "command-line automation\tarousal=1.00\nexecute system processes\tarousal=1.00\nmcp_tool:shell_exec__execute\tarousal=1.00\nrun shell commands\tarousal=1.00\nおやすみ\tarousal=0.67\nおやすみ気分\tarousal=0.67\n夜更かし\tarousal=0.67\n睡眠（夜／おやすみ）\tarousal=0.67\n2026-03-06: きのが夜更かししたが、明日つきのツールを増やせそうなのでまた明日と言って会話を締めた。\tarousal=0.34\nツールを増やす\tarousal=0.34\n明日また\tarousal=0.34\nきの\tarousal=0.33\n就寝\tarousal=0.33\nスマホを置く\tarousal=0.23\n囁き声（ASMR）\tarousal=0.23\n安心させる返答\tarousal=0.23",
  "active_state_limit": 16,
  "query_text": "サンドボックスのシェルを使えるようにした！ わかる？",
  "result_concepts": [
    "おやすみ気分",
    "おやすみ",
    "run shell commands",
    "ツールを増やす",
    "mcp_tool:shell_exec__execute",
    "パスワードマネージャー",
    "セキュリティツール",
    "夜／おやすみ",
    "仕事の達成感",
    "おやすみの見送り",
    "ファイルシステム",
    "論理削除",
    "つき",
    "色味の変化",
    "社員旅行",
    "Ghost in the Shell"
  ],
  "selected_seeds": [
    "run shell commands",
    "mcp_tool:shell_exec__execute",
    "ファイルシステム"
  ]
}
```

（寝る前に話しがちなのが分かり気まずい。）
ここで `mcp_tool:shell_exec__execute` が他の概念と同じように活性化して、`mcp_visible_tools` に現れているのが面白いところ。つまり「行為」も概念グラフの外にある特別なものではなく、連想の延長として見えてくる。その結果として、概念グラフ上の表現から動的ロードへ繋がる実装は「動的ロード」の理想の姿ではないかと思う。
