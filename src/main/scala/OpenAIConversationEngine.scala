import collection.JavaConverters._
import scala.jdk.OptionConverters._
import com.openai.client.okhttp.OpenAIOkHttpClient
import com.openai.models.ChatCompletionCreateParams
import com.openai.models.ChatModel
import com.openai.models.ChatCompletion
import com.openai.models.ChatCompletionMessage
import com.openai.models.CompletionUsage
import com.openai.core.JsonValue
import scala.collection.mutable.ArrayBuffer
import com.openai.models.ChatCompletionDeveloperMessageParam
import com.openai.models.ChatCompletionUserMessageParam
import com.openai.models.ChatCompletionMessageParam
import java.util.Optional

enum Role:
  case Developer, User, Assistant

class OpenAIConversationEngine(apiKey: String) extends ConversationEngine {
  val client =
    OpenAIOkHttpClient.builder()
    .apiKey(apiKey)
    .build()
  val history = ArrayBuffer(
    (Role.Developer, instruction)
  )

  def chat(content: String): String =
    history += ((Role.User, content))
    var paramsBuilder =
      ChatCompletionCreateParams.builder()
      .model(ChatModel.GPT_4O_MINI)
      .store(true)
    for (message <- history)
      val (role, content) = message
      paramsBuilder =
        role match {
          case Role.Developer => paramsBuilder.addDeveloperMessage(content)
          case Role.User => paramsBuilder.addUserMessage(content)
          case Role.Assistant => paramsBuilder.addMessage(
            ChatCompletionMessage
            .builder()
            .refusal(java.util.Optional.empty())
            .content(content)
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
    val response = message.content().get()
    history += ((Role.Assistant, response))
    response
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
