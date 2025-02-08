trait ConversationEngine {
  def chat(history: Seq[MessageRecord]): Either[ProgramError, MessageRecord]
}
