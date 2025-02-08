import scala.collection.mutable.ArrayBuffer
import java.nio.file.Files
import java.nio.file.StandardOpenOption
import java.nio.file.Paths
import java.io.File
import upickle.default._
import scala.io.Source
import scala.util.Using
import java.io.PrintWriter
import java.nio.charset.Charset
import java.io.FileWriter

case class MessageRecord(role: String, name: String, message: Message)

object MessageRecord {
  implicit val messageRW: ReadWriter[MessageRecord] = readwriter[ujson.Value].bimap[MessageRecord](
    {
      case MessageRecord(role, name, message) =>
        val messageJson = message match {
          case u: UserMessage      => writeJs(u)
          case a: AssistantMessage => writeJs(a)
        }
        messageJson.obj.remove("$type")
        ujson.Obj("role" -> role, "name" -> name, "message" -> messageJson)
    },
    json => {
      val role = json("role").str
      val messageJson = json("message")
      val message = role match {
        case "assistant" =>
          messageJson("$type") = classOf[AssistantMessage].getName()
          read[AssistantMessage](messageJson)
        case _ =>
          messageJson("$type") = classOf[UserMessage].getName()
          read[UserMessage](messageJson)
      }
      MessageRecord(role, json("name").str, message)
    }
  )
}

class MessageRepository(path: String, persist: Boolean) extends AutoCloseable {
  private val data = load(path)
  private val writer = Option.when(persist)(new PrintWriter(FileWriter(path, Charset.forName("UTF-8"), true)))

  private def load(path: String): ArrayBuffer[MessageRecord] = {
    val buf = new ArrayBuffer[MessageRecord]
    val source = Source.fromFile(path, "UTF-8")
    source.getLines().foreach { l =>
      val record = read[MessageRecord](l)
      println("loaded: " + record.toString())
      buf.addOne(record)
    }
    source.close()
    buf
  }

  def getAll(): IndexedSeq[MessageRecord] =
    data.toIndexedSeq

  def append(record: MessageRecord): Unit =
    println("append: " + record.toString())
    data.addOne(record)
    writer.foreach { w =>
      w.println(write(record))
      w.flush()
    }

  def close(): Unit =
    writer.foreach { w =>
      w.close()
    }
}
