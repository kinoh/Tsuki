import net.dv8tion.jda.api.hooks.ListenerAdapter
import net.dv8tion.jda.api.events.message.MessageReceivedEvent
import java.time.format.DateTimeFormatter
import java.time.Instant
import scala.util.Success
import scala.util.Failure
import scala.concurrent.ExecutionContext.Implicits.global
import java.util.concurrent.BlockingQueue

class DiscordEventListener(val queue: BlockingQueue[Event]) extends ListenerAdapter {

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
      queue.put(UserMessageEvent(message, name, event.getChannel().getId()))
    }
  
  private def isInSpecifiedChannel(event: MessageReceivedEvent): Boolean =
    scala.util.Properties.envOrElse("DISCORD_CHANNEL", "") match {
      case "" => true
      case id => event.getChannel().getId() == id
    }
}
