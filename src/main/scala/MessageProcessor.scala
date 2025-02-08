import java.time.Instant

class MessageProcessor(val engine: ConversationEngine, val repository: MessageRepository) {
  def initializeIfEmpty(): Unit =
    if (repository.getAll().length == 0)
      val timestamp = Instant.now()
      repository.append(MessageRecord("developer", "", UserMessage(timestamp.getEpochSecond(), instruction)))

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
{"feeling":5,"activity":5,"message":"ありがと💕"}
</example>
<example>
{"feeling":1,"activity":5,"message":"そんなこと言っちゃダメ！😠"}
</example>
<example>
{"feeling":5,"activity":1,"message":"癒されるよね…😊"}
</example>
<example>
{"feeling":1,"activity":1,"message":"無理かも…"}
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
