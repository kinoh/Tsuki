class MessageProcessor(val engine: ConversationEngine, val repository: MessageRepository) {
  def response(message: String): Either[AssistantMessageParseError, String] =
    repository.append(MessageRecord("user", message))
    val response = engine.chat(repository.getAll())
    repository.append(response)
    parseAssistantMessage(response.content).map(x => x.message)
}
