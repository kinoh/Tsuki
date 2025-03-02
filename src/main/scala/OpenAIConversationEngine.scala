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

  def chat(history: Seq[MessageRecord]): MessageRecord =
    var paramsBuilder =
      ChatCompletionCreateParams.builder()
      .model(ChatModel.GPT_4O)
      .store(true)
    for (record <- history)
      paramsBuilder =
        record.role match {
          case "user" =>
            paramsBuilder.addUserMessage(record.message)
          case "developer" =>
            paramsBuilder.addDeveloperMessage(record.message)
          case "assistant" =>
            paramsBuilder.addMessage(
              ChatCompletionMessage.builder()
                .refusal(java.util.Optional.empty())
                .content(record.message)
                .build()
            )
          case r =>
            scribe.warn("unknown role", scribe.data("role", r))
            paramsBuilder
        }
    val completion = client.chat().completions().create(paramsBuilder.build())

    completion.usage().toScala match
      case Some(x: CompletionUsage) =>
        scribe.info("token usage", scribe.data(Map(
          "prompt"     -> x.promptTokens(),
          "completion" -> x.completionTokens(),
        )))
      case None =>
        scribe.warn("no usage")

    val message = completion.choices().asScala.head.message()
    val role = message._role().toString()
    val content = message.content().get()

    MessageRecord(role, "", content)
}
