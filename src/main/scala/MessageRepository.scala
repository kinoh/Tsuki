import scala.collection.mutable.ArrayBuffer
import java.nio.file.Files
import java.nio.file.StandardOpenOption
import java.nio.file.Paths
import com.github.tototoshi.csv.CSVReader
import com.github.tototoshi.csv.CSVWriter
import java.io.File

case class MessageRecord(role: String, content: String)

class MessageRepository(path: String, persist: Boolean) extends AutoCloseable {
  private val data = load(path)
  private val writer = CSVWriter.open(new File(path), append = true)

  private def load(path: String): ArrayBuffer[MessageRecord] = {
    val buf = new ArrayBuffer[MessageRecord]
    val reader = CSVReader.open(new File(path))
    buf.addAll(
      reader.all().map(record =>
        val List(role, content) = record
        MessageRecord(role, content)))
    reader.close()
    buf
  }

  def getAll(): IndexedSeq[MessageRecord] =
    data.toIndexedSeq

  def append(record: MessageRecord): Unit =
    data.addOne(record)
    if (persist)
      writer.writeRow(Seq(record.role, record.content))
      writer.flush()

  def close(): Unit =
    writer.close()
}
