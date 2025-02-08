class DummyConversationEngine extends ConversationEngine {
  def chat(history: Seq[MessageRecord]): MessageRecord =
    MessageRecord("assistant", "{\"feeling\":0,\"activity\":0,\"message\":\"%s\"}".format(history.last.content))
}
