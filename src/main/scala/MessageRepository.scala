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

case class MessageRecord(role: String, name: String, message: String)

object MessageRecord {
  implicit val rw: ReadWriter[MessageRecord] = macroRW
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
