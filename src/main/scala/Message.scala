import upickle.default._

case class UserMessage(timestamp: Long, content: String)
object UserMessage {
  implicit val rw: ReadWriter[UserMessage] = macroRW
}

def encodeUserMessage(message: UserMessage): String =
  write(message)

case class AssistantMessage(feeling: Int, activity: Int, content: String)
object AssistantMessage {
  implicit val rw: ReadWriter[AssistantMessage] = macroRW
}

def encodeAssistantMessage(message: AssistantMessage): String =
  write(message)

def parseAssistantMessage(data: ujson.Value): Either[MessageParseError, AssistantMessage] =
  try {
    Right(read[AssistantMessage](data))
  }
  catch {
    case err => Left(MessageParseError(err.toString()))
  }

case class AssistantCode(code: String)
object AssistantCode {
  implicit val rw: ReadWriter[AssistantCode] = macroRW
}

def parseAssistantCode(data: ujson.Value): Either[MessageParseError, AssistantCode] =
  try {
    Right(read[AssistantCode](data))
  }
  catch {
    case err => Left(MessageParseError(err.toString()))
  }
