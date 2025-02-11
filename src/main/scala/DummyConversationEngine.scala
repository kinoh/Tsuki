import upickle.default.write
import upickle.default.read
import scala.util.Try

class DummyConversationEngine extends ConversationEngine {
  def chat(history: Seq[MessageRecord]): MessageRecord =
    val patterns = Map(
      "#code-time"  -> AssistantCode("import datetime\nprint(datetime.datetime.utcnow())"),
      "#code-sleep" -> AssistantCode("import time\ntime.sleep(5)\nprint(\"it's time\")"),
      "#code-req"   -> AssistantCode("import requests\nr = requests.get(\"https://example.com\")\nprint(r.text)"),
      "#code-err"   -> AssistantCode("print("),
    )

    val message = patterns.map { (key, value) =>
      Option.when(history.last.message.contains(key)) {
        write(value)
      }
    }.collectFirst {
      case Some(value) => value
    }.orElse(
      Try(read[UserMessage](history.last.message))
        .toOption
        .map(m => write(AssistantMessage(0, 0, m.content)))
    ).getOrElse(write(AssistantMessage(0, 0, "🤔")))

    MessageRecord("assistant", "", message)
}
