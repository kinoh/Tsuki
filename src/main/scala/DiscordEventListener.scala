import net.dv8tion.jda.api.hooks.ListenerAdapter
import net.dv8tion.jda.api.events.message.MessageReceivedEvent
import java.time.format.DateTimeFormatter

class DiscordEventListener(val engine: ConversationEngine) extends ListenerAdapter {

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
      val responseJson = engine.chat(message)
      val response = parseAssistantMessage(responseJson)
      response match {
        case Some(r) => event.getChannel().sendMessage(r.message).complete()
        case None => println("failed to parse: " + responseJson)
      }
    }
}
