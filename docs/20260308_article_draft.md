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
