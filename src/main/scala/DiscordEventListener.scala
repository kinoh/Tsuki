import net.dv8tion.jda.api.hooks.ListenerAdapter
import net.dv8tion.jda.api.events.message.MessageReceivedEvent
import java.time.format.DateTimeFormatter

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
    if (!event.getAuthor.isBot && event.getChannel().getId() == "1337074638307594280") {
      processor.response(message) match {
        case Right(r) => event.getChannel().sendMessage(r).complete()
        case Left(err) => println("failed to parse: " + err)
      }
    }
}
