import org.vosk.LibVosk
import org.vosk.LogLevel
import org.vosk.Model
import org.vosk.Recognizer
import upickle.default.*

import java.util.concurrent.BlockingQueue
import java.util.concurrent.TimeUnit

case class AudioNotification(size: Int, user: String)

case class VoskRecognitionResult(text: String)
object VoskRecognitionResult {
  implicit val rw: ReadWriter[VoskRecognitionResult] = macroRW
}

class VoskSpeechRecognizer(val sampleRate: Int, val modelPath: String) {
  private val model = Model(modelPath)

  def run(sharedBuffer: Array[Short], audioNotifier: BlockingQueue[AudioNotification], eventQueue: BlockingQueue[Event]): Unit =
    System.setProperty("jna.encoding", "UTF-8")
    LibVosk.setLogLevel(LogLevel.DEBUG)

    val core = Recognizer(model, sampleRate)
    val buffer = new Array[Short](sharedBuffer.length)
    val empty = new Array[Short](sampleRate * 100 / 1000)
    var speakingUser: Option[String] = None

    while true do
      val finished =
        Option(audioNotifier.poll(100, TimeUnit.MILLISECONDS)) match
          case None =>
            if speakingUser.isDefined then
              core.acceptWaveForm(empty, empty.length)
            else
              false
          case Some(n) =>
            if speakingUser.isEmpty then
              scribe.debug("hearing...", scribe.data("user", n.user))
            speakingUser = Some(n.user)
            sharedBuffer.synchronized {
              System.arraycopy(sharedBuffer, 0, buffer, 0, n.size)
            }
            core.acceptWaveForm(buffer, n.size)
      if finished then
        val result = core.getResult()
        val data = read[VoskRecognitionResult](result)
        scribe.debug("recognition result", scribe.data("text", data.text))
        if !data.text.isEmpty() then
          eventQueue.put(UserMessageEvent(speakingUser.getOrElse("unknown"), "text", data.text))
        speakingUser = None
}
