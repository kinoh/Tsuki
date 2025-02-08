trait ConversationEngine {
  def chat(history: Seq[MessageRecord]): MessageRecord
}
