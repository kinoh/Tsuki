import net.dv8tion.jda.api.hooks.ListenerAdapter
import net.dv8tion.jda.api.events.message.MessageReceivedEvent
import java.time.format.DateTimeFormatter
import java.time.Instant
import scala.util.Success
import scala.util.Failure
import scala.concurrent.ExecutionContext.Implicits.global

class DiscordEventListener(val processor: MessageProcessor) extends ListenerAdapter {

  override def onMessageReceived(event: MessageReceivedEvent): Unit =
    val message = event.getMessage().getContentDisplay()
    println("%s, %s, %s, %s, %s, %s, %s".format(
      event.getAuthor().getId(),
      event.getAuthor().getName(),
      event.getAuthor().getEffectiveName(),
      event.getChannel().getName(),
      event.getChannel().getId(),
      event.getMessage().getTimeCreated().format(DateTimeFormatter.ISO_OFFSET_DATE_TIME),
      message
    ))
    if (!event.getAuthor.isBot && isInSpecifiedChannel(event)) {
      val name = event.getAuthor().getEffectiveName()
      processor.receive(message, name, Instant.now()).onComplete {
        case Success(Right(Some(content))) => event.getChannel().sendMessage(content).complete()
        case Success(Right(None))          => println("no response")
        case Success(Left(err))            => println("processor error: " + err)
        case Failure(t)                    => println("processor future error: " + t)
      }
    }
  
  private def isInSpecifiedChannel(event: MessageReceivedEvent): Boolean =
    scala.util.Properties.envOrElse("DISCORD_CHANNEL", "") match {
      case "" => true
      case id => event.getChannel().getId() == id
    }
}
