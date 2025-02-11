import java.nio.file.Files
import java.io.FileWriter
import java.nio.charset.Charset
import scala.util.Using
import java.io.FileReader
import scala.io.Source

class MessageRepositoryTest extends munit.FunSuite {

  test("MessageRepository.loadAndGet") {
    val path = Files.createTempFile("tmp", null).toString()
    Using(FileWriter(path, Charset.forName("UTF-8"))) { writer =>
      writer.write("""{"role":"developer","name":"","message":"{\"timestamp\":1735754645,\"content\":\"あなたはかわいい口調ながら完璧な知性を備えた雑談エージェントです。\\n<example>ユーザー「こんにちは」あなた「こんにちはっ✨」</example>\"}"}
{"role":"user","name":"me","message":"{\"timestamp\":1735754646,\"content\":\"hi\"}"}
{"role":"assistant","name":"","message":"{\"feeling\":5,\"activity\":4,\"content\":\"hi!\"}"}
""")
    }
    val repository = MessageRepository(path, false)
    val expected = IndexedSeq(
      MessageRecord("developer", "", "{\"timestamp\":1735754645,\"content\":\"あなたはかわいい口調ながら完璧な知性を備えた雑談エージェントです。\\n<example>ユーザー「こんにちは」あなた「こんにちはっ✨」</example>\"}"),
      MessageRecord("user", "me", "{\"timestamp\":1735754646,\"content\":\"hi\"}"),
      MessageRecord("assistant", "", "{\"feeling\":5,\"activity\":4,\"content\":\"hi!\"}")
    )
    assertEquals(repository.getAll(), expected)
  }

  test("MessageRepository.getDeveloperAndRecent") {
    val path = Files.createTempFile("tmp", null).toString()
    val repository = MessageRepository(path, true)

    val records = Seq(
      MessageRecord("developer", "", "いろはにほへと"),
      MessageRecord("user", "1", "ちりぬるを"),
      MessageRecord("user", "2", "わかよたれそ"),
      MessageRecord("user", "3", "つねならむ"),
      MessageRecord("user", "4", "うゐのおくやま"),
      MessageRecord("user", "5", "けふこへて")
    )
    records.foreach { record =>
      repository.append(record)
    }
    assertEquals(repository.getAll(), records)
    assertEquals(repository.getDeveloperAndRecent(2), records.headOption.toIndexedSeq ++ records.takeRight(2).toIndexedSeq)

    repository.close()
  }

  test("MessageRepository.append") {
    val path = Files.createTempFile("tmp", null).toString()
    val repository = MessageRepository(path, false)
    assertEquals(repository.getAll(), IndexedSeq())

    val newRecord = MessageRecord("user", "I", "{\"timestamp\":1735754645,\"content\":\"こんにちは😊\"}")
    repository.append(newRecord)
    assertEquals(repository.getAll(), IndexedSeq(newRecord))

    val obtained = Using(Source.fromFile(path, "UTF-8")) { source => source.mkString }.getOrElse("")
    assertEquals(obtained, "")

    repository.close()
  }

  test("MessageRepository.appendPersist") {
    val path = Files.createTempFile("tmp", null).toString()
    val repository = MessageRepository(path, true)
    assertEquals(repository.getAll(), IndexedSeq())

    val newRecord = MessageRecord("user", "I", "{\"timestamp\":1735754645,\"content\":\"こんにちは😊\"}")
    repository.append(newRecord)
    assertEquals(repository.getAll(), IndexedSeq(newRecord))

    val obtained = Using(Source.fromFile(path, "UTF-8")) { source => source.mkString }.getOrElse("")
    assertEquals(obtained, """{"role":"user","name":"I","message":"{\"timestamp\":1735754645,\"content\":\"こんにちは😊\"}"}
""")

    repository.close()
  }

  test("MessageRepository.rewriteDeveloperPrompt") {
    val path = Files.createTempFile("tmp", null).toString()
    val repository = MessageRepository(path, false)

    val developerRecord = MessageRecord("developer", "", "{\"timestamp\":1735754645,\"content\":\"You're driving assistant.\"}")
    val userRecord = MessageRecord("user", "bar", "{\"timestamp\":1735754646,\"content\":\"佐渡島\"}")
    repository.append(developerRecord)
    repository.append(userRecord)

    val newDeveloperRecord = MessageRecord("developer", "", "{\"timestamp\":1735754647,\"content\":\"You're travel assistant.\"}")
    repository.rewriteDeveloperPrompt(newDeveloperRecord)
    assertEquals(repository.getAll(), IndexedSeq(newDeveloperRecord, userRecord))

    repository.close()

    val obtained = Using(Source.fromFile(path, "UTF-8")) { source => source.mkString }.getOrElse("")
    assertEquals(obtained, "")
  }

  test("MessageRepository.rewriteDeveloperPromptPersist") {
    val path = Files.createTempFile("tmp", null).toString()
    val repository = MessageRepository(path, true)

    val developerRecord = MessageRecord("developer", "", "{\"timestamp\":1735754645,\"content\":\"You're driving assistant.\"}")
    val userRecord = MessageRecord("user", "bar", "{\"timestamp\":1735754646,\"content\":\"佐渡島\"}")
    repository.append(developerRecord)
    repository.append(userRecord)

    val newDeveloperRecord = MessageRecord("developer", "", "{\"timestamp\":1735754647,\"content\":\"You're travel assistant.\"}")
    repository.rewriteDeveloperPrompt(newDeveloperRecord)
    assertEquals(repository.getAll(), IndexedSeq(newDeveloperRecord, userRecord))

    repository.close()

    val obtained = Using(Source.fromFile(path, "UTF-8")) { source => source.mkString }.getOrElse("")
    assertEquals(obtained, """{"role":"developer","name":"","message":"{\"timestamp\":1735754647,\"content\":\"You're travel assistant.\"}"}
{"role":"user","name":"bar","message":"{\"timestamp\":1735754646,\"content\":\"佐渡島\"}"}
""")
  }
}
