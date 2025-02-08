import net.dv8tion.jda.api.hooks.ListenerAdapter
import net.dv8tion.jda.api.events.message.MessageReceivedEvent
import java.time.format.DateTimeFormatter
import java.time.Instant

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
      processor.receive(message, name, Instant.now()) match {
        case Right(r) => event.getChannel().sendMessage(r).complete()
        case Left(err) => println("failed to parse: " + err)
      }
    }
  
  private def isInSpecifiedChannel(event: MessageReceivedEvent): Boolean =
    scala.util.Properties.envOrElse("DISCORD_CHANNEL", "") match {
      case "" => true
      case id => event.getChannel().getId() == id
    }
}
