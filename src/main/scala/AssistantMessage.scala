import play.api.libs.json._

case class AssistantMessage(feeling: Int, activity: Int, message: String)
object AssistantMessage {
    implicit val jsonReads: Reads[AssistantMessage] = Json.reads[AssistantMessage]
}

def parseAssistantMessage(data: String): Option[AssistantMessage] =
  Json
  .parse(data)
  .validate[AssistantMessage]
  .asOpt
