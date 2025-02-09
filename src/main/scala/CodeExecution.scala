import upickle.default._
import sttp.client4.quick.*
import sttp.model.StatusCode
import scala.util.Try
import scala.util.Failure
import scala.util.Success
import sttp.model.Uri

case class DifyRunResultData(error: String, stdout: String)
object DifyRunResultData {
  implicit val rw: ReadWriter[DifyRunResultData] = macroRW
}

case class DifyRunResult(code: Int, message: String, data: Option[DifyRunResultData])
object DifyRunResult {
  implicit val rw: ReadWriter[DifyRunResult] = macroRW
}

def executeCode(code: String): Either[ProgramError, String] =
  val json = ujson.Obj(
    "language"       -> "python3",
    "code"           -> code,
    "enable_network" -> true
  )
  val endpointUri = scala.util.Properties.envOrElse("DIFY_SANDBOX_HOST", "") match {
    case ""   => return Left(AssertionError("DIFY_SANDBOX_HOST not set"))
    case host =>
      Uri.parse(s"http://$host/v1/sandbox/run") match {
        case Left(err)  => return Left(AssertionError("DIFY_SANDBOX_HOST is invalid: " + err))
        case Right(uri) => uri
      }
  }
  val apiKey = scala.util.Properties.envOrElse("DIFY_SANDBOX_API_KEY", "") match {
    case ""    => return Left(AssertionError("DIFY_SANDBOX_API_KEY not set"))
    case value => value
  }
  val response =
    quickRequest
    .post(endpointUri)
    .header("Content-Type", "application/json")
    .header("X-Api-Key", apiKey)
    .body(ujson.write(json))
    .send()
  Right(response)
    .flatMap { r =>
      if (response.code.isSuccess) Right(r)
      else Left(HttpRequestError("response code=" + response.code))
    }
    .flatMap { r =>
      Try(read[DifyRunResult](response.body)) match {
        case Success(result) => Right(result)
        case Failure(exception) => Left(JsonParseError(response.body))
      }
    }
    .flatMap { r =>
      r.data match {
        case Some(data) =>
          if (data.error == "") Right(data.stdout)
          else Left(CodeExecutionError(r.code, r.message, Some(data.error)))
        case None => Left(CodeExecutionError(r.code, r.message, None))
      }
    }
