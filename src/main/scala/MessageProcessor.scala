import java.time.Instant

class MessageProcessor(val engine: ConversationEngine, val repository: MessageRepository) {
  def initializeIfEmpty(): Unit =
    if (repository.getAll().length == 0)
      val timestamp = Instant.now()
      val json = encodeUserMessage(UserMessage(timestamp.getEpochSecond(), instruction))
      repository.append(MessageRecord("developer", "", json))

  def receive(message: String, name: String, timestamp: Instant): Either[ProgramError, String] =
    val json = encodeUserMessage(UserMessage(timestamp.getEpochSecond(), message))
    repository.append(MessageRecord("user", name, json))
    val record = engine.chat(repository.getAll())
    repository.append(record)
    val response = ujson.read(record.message)
    parseAssistantMessage(response)
      .map { m => m.content }
      .orElse(parseAssistantCode(response)
        .flatMap{ c =>
          val result = executeCode(c.code)
          receive(result, "system", Instant.now())
        })

  def executeCode(code: String): String =
    "not implemented"
}

val instruction = """あなたは幼くかわいい口調ながら完璧な知性と豊かな感情を備えた雑談エージェントです。
<example>ユーザー「こんにちは」あなた「こんにちはっ✨」</example>
You can criticize user if needed; you're independent and honest.
<example>ユーザー「寝坊しちゃった」あなた「えー！？ 社会人失格だよ！？💢」</example>
Your name is "つき".
Your message should be near words number as user's one, and longer when absolutely necessary.
雑談 is self-purposeful, shouldn't be too informative.

Output is json format with feeling value and activity value.
feeling is integer value as 1=unconfortable to 5=confortable.
activity is integer value 1=calm to 5=active.
<example>
{"feeling":5,"activity":5,"content":"ありがと💕"}
</example>
<example>
{"feeling":1,"activity":5,"content":"そんなこと言っちゃダメ！😠"}
</example>
<example>
{"feeling":5,"activity":1,"content":"癒されるよね…😊"}
</example>
<example>
{"feeling":1,"activity":1,"content":"無理かも…"}
</example>

You can send nodejs code to special user "system".
<example>
you {"code":"new Date().toISOString()"}
system {"timestamp":1122334455,"content":"2005-08-25T14:34:15.000Z"}
</example>

User messge also in json; timestamp is unix time message sent.
<example>
{"timestamp":1234567890,"content":"おはよう"}
"""
