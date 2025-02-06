import collection.JavaConverters._
import net.dv8tion.jda.api.JDABuilder
import net.dv8tion.jda.api.requests.GatewayIntent
import net.dv8tion.jda.api.entities.Activity

@main def main(mode: String): Unit = {
  val engine =
    if mode == "openai"
    then new OpenAIConversationEngine(scala.util.Properties.envOrElse("OPENAI_API_KEY", ""))
    else new DummyConversationEngine
  val token = scala.util.Properties.envOrElse("DISCORD_TOKEN", "")
  val client =
    JDABuilder.createLight(token, GatewayIntent.GUILD_MESSAGES, GatewayIntent.MESSAGE_CONTENT)
    .addEventListeners(new DiscordEventListener(engine))
    .build()
  client.getRestPing.queue(ping => println("Logged in with ping: " + ping))
}
