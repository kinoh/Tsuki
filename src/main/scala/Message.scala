import upickle.default._

sealed trait Message

case class UserMessage(timestamp: Long, content: String) extends Message
object UserMessage {
  implicit val rw: ReadWriter[UserMessage] = macroRW
}

def encodeUserMessage(message: UserMessage): String =
  val js = writeJs(message)
  js.obj.remove("$type")
  write(js)

case class AssistantMessage(feeling: Int, activity: Int, content: String) extends Message
object AssistantMessage {
  implicit val rw: ReadWriter[AssistantMessage] = macroRW
}

def encodeAssistantMessage(message: AssistantMessage): String =
  val js = writeJs(message)
  js.obj.remove("$type")
  write(js)

def parseAssistantMessage(data: String): Either[MessageParseError, AssistantMessage] =
  try {
    val js = ujson.read(data)
    js("$type") = classOf[AssistantMessage].getName()
    Right(read[AssistantMessage](js))
  }
  catch {
    case err => Left(MessageParseError(err.toString()))
  }
