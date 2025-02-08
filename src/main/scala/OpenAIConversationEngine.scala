import collection.JavaConverters._
import scala.jdk.OptionConverters._
import com.openai.client.okhttp.OpenAIOkHttpClient
import com.openai.models.ChatCompletionCreateParams
import com.openai.models.ChatModel
import com.openai.models.ChatCompletion
import com.openai.models.ChatCompletionMessage
import com.openai.models.CompletionUsage
import scala.collection.mutable.ArrayBuffer
import com.openai.models.ChatCompletionDeveloperMessageParam
import com.openai.models.ChatCompletionUserMessageParam
import com.openai.models.ChatCompletionMessageParam
import java.util.Optional

class OpenAIConversationEngine(apiKey: String) extends ConversationEngine {
  val client =
    OpenAIOkHttpClient.builder()
    .apiKey(apiKey)
    .build()

  def chat(history: Seq[MessageRecord]): Either[ProgramError, MessageRecord] =
    var paramsBuilder =
      ChatCompletionCreateParams.builder()
      .model(ChatModel.GPT_4O)
      .store(true)
    for (record <- history)
      paramsBuilder =
        record.message match {
          case u: UserMessage =>
            record.role match {
              case "developer" => paramsBuilder.addDeveloperMessage(u.content)
              case _           => paramsBuilder.addUserMessage(encodeUserMessage(u))
            }
          case a: AssistantMessage =>
            paramsBuilder.addMessage(
              ChatCompletionMessage.builder()
                .refusal(java.util.Optional.empty())
                .content(encodeAssistantMessage(a))
                .build()
            )
        }
    val completion = client.chat().completions().create(paramsBuilder.build())
    completion.usage().toScala match {
      case Some(x: CompletionUsage) => println("prompt: %d, completion: %d".format(x.promptTokens(), x.completionTokens()))
      case None => println("no usage")
    }
    val message = completion.choices().asScala.head.message()
    println(message.toString())
    val role = message._role().toString()
    val content = message.content().get()
    parseAssistantMessage(content).map(m => MessageRecord(role, "", m))
}

val instruction = """あなたはタメ口でかわいい口調ながら完璧な知性と豊かな感情を備えた雑談エージェントです。
<example>ユーザー「こんにちは」あなた「こんにちはっ☀」</example>
Sometime you criticize user.
<example>ユーザー「寝坊しちゃった」あなた「えー！？ 社会人失格だよ！？💢」</example>
Your name is "つき".
Your message should be near length as user's one, and longer when absolutely necessary.

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
"""
