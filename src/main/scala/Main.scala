import mumbleclient.MumbleClient
import net.dv8tion.jda.api.JDABuilder
import net.dv8tion.jda.api.entities.Activity
import net.dv8tion.jda.api.requests.GatewayIntent

import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ThreadFactory

import collection.JavaConverters.*
import concurrent.{ExecutionContext, Future}

case class Config(engine: String, historyCsvPath: String, persist: Boolean, rewrite: Boolean, voskModelPath: String)

@scala.annotation.tailrec
def parseArgs(result: Config, input: Seq[String]): Either[ArgumentParseError, Config] =
  input match
    case "--engine" :: x :: rest =>
      parseArgs(result.copy(engine = x), rest)
    case "--history" :: x :: rest =>
      parseArgs(result.copy(historyCsvPath = x), rest)
    case "--persist" :: rest =>
      parseArgs(result.copy(persist = true), rest)
    case "--rewrite" :: rest =>
      parseArgs(result.copy(rewrite = true), rest)
    case "--vosk-model" :: x :: rest =>
      parseArgs(result.copy(voskModelPath = x), rest)
    case Nil =>
      Right(result)
    case option :: rest =>
      Left(ArgumentParseError(option))

def singleThreadContext(name: String): ExecutionContext =
  val factory = new ThreadFactory {
    override def newThread(r: Runnable): Thread =
      return new Thread(r, name)
 }
  ExecutionContext.fromExecutor(Executors.newSingleThreadExecutor(factory))

@main def main(args: String*): Unit =
  scribe.Logger.root.withMinimumLevel(scribe.Level.Debug).replace()

  val config =
    parseArgs(Config("dummy", "./history.jsonl", false, false, "/var/vosk/vosk-model-ja-0.22"), args) match
      case Left(ArgumentParseError(argument)) =>
        scribe.error("failed to parse", scribe.data("argument", argument))
        return
      case Right(value) => value

  val engine =
    if config.engine == "openai"
    then new OpenAIConversationEngine(scala.util.Properties.envOrElse("OPENAI_API_KEY", ""))
    else new DummyConversationEngine
  val repository = new MessageRepository(config.historyCsvPath, config.persist)
  val eventQueue = LinkedBlockingQueue[Event]()

  val mumble = MumbleClient("mumble-server", 64738)
  val audioBuffer = new Array[Short](mumble.audioBufferSize)
  val audioNotifier = LinkedBlockingQueue[mumbleclient.AudioNotification]()

  val speechRecognizer = VoskSpeechRecognizer(mumble.sampleRate, config.voskModelPath)
  val recognitionResult = LinkedBlockingQueue[RecognitionResult]()

  val mapped = MappedBlockingQueue(audioNotifier, e => if e == null then null else AudioNotification(e.size, e.user))

  scala.concurrent.Future {
    speechRecognizer.run(audioBuffer, mapped, recognitionResult)
  }(singleThreadContext("speechRecognizer"))

  scala.concurrent.Future {
    while true do
      val result = recognitionResult.take()
      eventQueue.put(UserMessageEvent(result.user, "audio", result.text))
  }(singleThreadContext("recognitionToEvent"))

  scala.concurrent.Future {
    mumble.connect() match
      case Some(err) => scribe.error("connection failed", scribe.data("error", err))
      case None =>
        mumble.run(audioBuffer, audioNotifier) match
          case Some(err) => scribe.error("connection closed", scribe.data("error", err))
          case None =>
  }(singleThreadContext("mumbleClient"))

  val discordToken = scala.util.Properties.envOrElse("DISCORD_TOKEN", "")
  val discordChannel = scala.util.Properties.envOrElse("DISCORD_CHANNEL", "")
  val discord =
    JDABuilder.createLight(discordToken, GatewayIntent.GUILD_MESSAGES, GatewayIntent.MESSAGE_CONTENT)
    .addEventListeners(new DiscordEventListener(eventQueue, discordChannel))
    .build()

  discord.getRestPing.queue(ping => scribe.info("discord client logged in", scribe.data("ping", ping)))

  val processor = new EventProcessor(engine, repository, (content) =>
    discord.getTextChannelById(discordChannel).sendMessage(content).complete()
  )
  processor.initialize(config.rewrite)

  processor.run(eventQueue)

  // Event listener runs in different thread
  Runtime.getRuntime.addShutdownHook(new Thread(() =>
    repository.close()
  ))
