import play.api.libs.json._

case class AssistantMessage(feeling: Int, activity: Int, message: String)
object AssistantMessage {
    implicit val jsonReads: Reads[AssistantMessage] = Json.reads[AssistantMessage]
}

def parseAssistantMessage(data: String): Option[AssistantMessage] =
  try {
    Json
    .parse(data)
    .validate[AssistantMessage]
    .asOpt
  }
  catch {
    case err =>
      println("parsing error: " + err.toString())
      None
  }
