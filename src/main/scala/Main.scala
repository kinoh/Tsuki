import mumbleclient.MumbleClient
import net.dv8tion.jda.api.JDABuilder
import net.dv8tion.jda.api.entities.Activity
import net.dv8tion.jda.api.requests.GatewayIntent

import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ThreadFactory
import scala.util.Failure
import scala.util.Success

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

def fixedThreadPoolContext(n: Int, name: String): ExecutionContext =
  val factory = new ThreadFactory {
    override def newThread(r: Runnable): Thread =
      return new Thread(r, name)
  }
  ExecutionContext.fromExecutor(Executors.newFixedThreadPool(n, factory))

def singleThreadContext(name: String): ExecutionContext =
  fixedThreadPoolContext(1, name)

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
    then OpenAIConversationEngine(scala.util.Properties.envOrElse("OPENAI_API_KEY", ""))
    else DummyConversationEngine()
  val repository = MessageRepository(config.historyCsvPath, config.persist)
  val eventQueue = LinkedBlockingQueue[Event]()

  val mumble = MumbleClient("mumble-server", 64738)
  val audioBuffer = new Array[Short](mumble.audioBufferSize)
  val audioNotifier = LinkedBlockingQueue[mumbleclient.AudioNotification]()

  val mapped = MappedBlockingQueue(audioNotifier, e => if e == null then null else AudioNotification(e.size, e.user))

  scala.concurrent.Future {
    val speechRecognizer = VoskSpeechRecognizer(mumble.sampleRate, config.voskModelPath)
    speechRecognizer.run(audioBuffer, mapped, eventQueue)
  }(singleThreadContext("SpeechRecognizer"))

  val mumbleContext = singleThreadContext("MumbleClient")
  scala.concurrent.Future {
    mumble.connect()
      .toLeft(())
      .left.map { err => s"connection failed: ${err}" }
      .flatMap { _ =>
        mumble.run(audioBuffer, audioNotifier)
          .toLeft(())
      }
      .left.map { err => s"connection closed: ${err}" }
  }(mumbleContext).onComplete {
    case Success(Right(_)) =>
    case Success(Left(err)) =>
      scribe.error("Mumble client failed", err)
    case Failure(exception) =>
      scribe.error("future failed", exception)
  }(mumbleContext)

  val discordToken = scala.util.Properties.envOrElse("DISCORD_TOKEN", "")
  val discordChannel = scala.util.Properties.envOrElse("DISCORD_CHANNEL", "")
  val discord =
    JDABuilder.createLight(discordToken, GatewayIntent.GUILD_MESSAGES, GatewayIntent.MESSAGE_CONTENT)
    .addEventListeners(DiscordEventListener(eventQueue, discordChannel))
    .build()

  discord.getRestPing.queue(ping => scribe.info("discord client logged in", scribe.data("ping", ping)))

  val processor = EventProcessor(engine, repository, (content) =>
    discord.getTextChannelById(discordChannel).sendMessage(content).complete()
  )
  processor.initialize(config.rewrite)

  processor.run(eventQueue, fixedThreadPoolContext(4, "EventProcessor"))

  // Event listener runs in different thread
  Runtime.getRuntime.addShutdownHook(new Thread(() =>
    repository.close()
  ))
