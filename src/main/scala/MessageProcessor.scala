class MessageProcessor(val engine: ConversationEngine) {
  def response(message: String): Either[AssistantMessageParseError, String] = {
    val responseJson = engine.chat(message)
    val response = parseAssistantMessage(responseJson)
    response match {
    case Some(r) => Right(r.message)
    case None => Left(AssistantMessageParseError(responseJson))
    }
  }
}
