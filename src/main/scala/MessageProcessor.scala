import java.time.Instant
class MessageProcessor(val engine: ConversationEngine, val repository: MessageRepository) {
  def response(message: String, name: String, timestamp: Instant): Either[ProgramError, String] =
    repository.append(MessageRecord("user", name, UserMessage(timestamp.getEpochSecond(), message)))
    engine.chat(repository.getAll())
      .flatMap(record =>
        repository.append(record)
        record.message match {
          case AssistantMessage(_, _, content) => Right(content)
          case _ => Left(AssertionError("Response message must be AssistantMessage"))
        })
}
