import upickle.default._

case class AssistantMessage(feeling: Int, activity: Int, message: String)
object AssistantMessage {
    implicit val rw: ReadWriter[AssistantMessage] = macroRW
}

def parseAssistantMessage(data: String): Either[AssistantMessageParseError, AssistantMessage] =
  try {
    Right(read[AssistantMessage](data))
  }
  catch {
    case err => Left(AssistantMessageParseError(err.toString()))
  }
