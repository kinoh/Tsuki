class DummyConversationEngine extends ConversationEngine {
  def chat(history: Seq[MessageRecord]): MessageRecord =
    MessageRecord("assistant", "", encodeAssistantMessage(AssistantMessage(0, 0, history.last.message)))
}
