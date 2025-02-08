class DummyConversationEngine extends ConversationEngine {
  def chat(history: Seq[MessageRecord]): Either[ProgramError, MessageRecord] =
    val content = history.last.message match {
      case u: UserMessage => u.content
      case _ => "no input"
    }
    Right(MessageRecord("assistant", "", AssistantMessage(0, 0, content)))
}
