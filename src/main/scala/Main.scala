import collection.JavaConverters._
import net.dv8tion.jda.api.JDABuilder
import net.dv8tion.jda.api.requests.GatewayIntent
import net.dv8tion.jda.api.entities.Activity
import java.util.concurrent.LinkedBlockingQueue

case class Config(engine: String, historyCsvPath: String, persist: Boolean, rewrite: Boolean)

@scala.annotation.tailrec
def parseArgs(result: Config, input: Seq[String]): Either[ArgumentParseError, Config] = {
  input match {
    case "--engine" :: x :: rest => {
      parseArgs(result.copy(engine = x), rest)
    }
    case "--history" :: x :: rest => {
      parseArgs(result.copy(historyCsvPath = x), rest)
    }
    case "--persist" :: rest => {
      parseArgs(result.copy(persist = true), rest)
    }
    case "--rewrite" :: rest => {
      parseArgs(result.copy(rewrite = true), rest)
    }
    case Nil => {
      Right(result)
    }
    case option :: rest => {
      Left(ArgumentParseError(option))
    }
  }
}

@main def main(args: String*): Unit = {
  val config =
    parseArgs(Config("dummy", "./history.jsonl", false, false), args) match {
      case Left(ArgumentParseError(argument)) =>
        println("invalid arg: " + argument)
        return
      case Right(value) => value
    }
  val engine =
    if config.engine == "openai"
    then new OpenAIConversationEngine(scala.util.Properties.envOrElse("OPENAI_API_KEY", ""))
    else new DummyConversationEngine
  val repository = new MessageRepository(config.historyCsvPath, config.persist)
  val queue = LinkedBlockingQueue[Event]()
  val token = scala.util.Properties.envOrElse("DISCORD_TOKEN", "")
  val client =
    JDABuilder.createLight(token, GatewayIntent.GUILD_MESSAGES, GatewayIntent.MESSAGE_CONTENT)
    .addEventListeners(new DiscordEventListener(queue))
    .build()
  val processor = new EventProcessor(engine, repository, (content, channel) => {
    client.getTextChannelById(channel).sendMessage(content).complete()
  })
  processor.initialize(config.rewrite)
  client.getRestPing.queue(ping => println("Logged in with ping: " + ping))

  processor.run(queue)

  // Event listener runs in different thread
  Runtime.getRuntime.addShutdownHook(new Thread(() =>
    repository.close()
  ))
}
