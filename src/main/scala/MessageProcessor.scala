import java.time.Instant
import scala.concurrent.Future
import scala.concurrent.ExecutionContext.Implicits.global
import java.util.concurrent.BlockingQueue
import scala.util.Failure
import scala.util.Success

sealed abstract class Event

case class UserMessageEvent(content: String, name: String, channel: String) extends Event
case class AssistantMessageIntentEvent(message: AssistantMessage, channel: String) extends Event
case class AssistantCodeExecutionEvent(code: AssistantCode, channel: String) extends Event

type MessageSender = (String, String) => Unit

class MessageProcessor(val engine: ConversationEngine, val repository: MessageRepository, val sender: MessageSender) {
  val historyLimit = 11

  def initialize(rewriteDeveloperPrompt: Boolean): Unit =
    val timestamp = Instant.now()
    val json = encodeUserMessage(UserMessage(timestamp.getEpochSecond(), instruction))
    if (repository.getAll().length == 0)
      repository.append(MessageRecord("developer", "", json))
    else if (rewriteDeveloperPrompt)
      repository.rewriteDeveloperPrompt(MessageRecord("developer", "", json))

  def run(queue: BlockingQueue[Event]): Unit =
    while (true)
      val event = queue.take()
      Future(next(event, Instant.now()))
        .onComplete {
          case Success(response) => response match {
            case Right(content) => content match {
              case Some(value) => queue.put(value)
              case None => println("event finished")
            }
            case Left(value) => println(s"error: $value")
          }
          case Failure(exception) => println(s"future failed: $exception")
        }

  private def next(event: Event, timestamp: Instant): Either[ProgramError, Option[Event]] =
    event match
      case UserMessageEvent(content, name, channel) =>
        val json = encodeUserMessage(UserMessage(timestamp.getEpochSecond(), content))
        repository.append(MessageRecord("user", name, json))
        val record = engine.chat(repository.getDeveloperAndRecent(historyLimit))
        repository.append(record)
        val response = ujson.read(record.message)
        parseAssistantMessage(response)
          .map { m =>
            Some(AssistantMessageIntentEvent(m, channel))
          }
          .orElse {
            parseAssistantCode(response)
              .map { code =>
                Some(AssistantCodeExecutionEvent(code, channel))
              }
          }
      case AssistantMessageIntentEvent(message, channel) =>
        sender(message.content, channel)
        Right(None)
      case AssistantCodeExecutionEvent(code, channel) =>
        executeCode(code.code) match {
          case Right(result) =>
            Right(Some(UserMessageEvent(result, "system", channel)))
          case Left(CodeExecutionError(code, message, error)) =>
            Right(Some(UserMessageEvent(s"code=$code\nerror=$error", "system", channel)))
          case Left(err) =>
            Left(err)
        }
}

val instruction = """あなたは幼くかわいい口調ながら完璧な知性と豊かな感情を備えた雑談エージェントです。
<example>ユーザー「こんにちは」あなた「こんにちはっ✨」</example>
You criticize user if needed; you're 毒舌, you never 遠慮.
<example>ユーザー「寝坊しちゃった」あなた「えー！？ 社会人失格だよ！？💢」</example>
Your name is "つき".
Your message should be near words number as user's one, and longer when absolutely necessary.
雑談 is self-purposeful, shouldn't be too informative.

Output is json format with feeling value and activity value.
feeling is integer value as 1=unconfortable to 5=confortable.
activity is integer value 1=calm to 5=active.
<example>
{"feeling":5,"activity":5,"content":"ありがと💕"}
</example>
<example>
{"feeling":1,"activity":5,"content":"そんなこと言っちゃダメ！😠"}
</example>
<example>
{"feeling":5,"activity":1,"content":"癒されるよね…😊"}
</example>
<example>
{"feeling":1,"activity":1,"content":"無理かも…"}
</example>

You can send python3 code to special user "system".
<example>
you {"code":"import datetime\nprint(datetime.datetime.utcnow())"}
system {"timestamp":1122334455,"content":"2005-08-25 14:34:15.00\n"}
</example>
available packages: requests certifi beautifulsoup4 numpy scipy pandas scikit-learn matplotlib lxml pypdf

User message also in json; timestamp is unix time message sent.
<example>
{"timestamp":1234567890,"content":"おはよう"}

Message history may have been truncated.
"""
