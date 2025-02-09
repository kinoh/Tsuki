sealed trait ProgramError
case class JsonParseError(message: String) extends ProgramError
case class ArgumentParseError(argument: String) extends ProgramError
case class AssertionError(detail: String) extends ProgramError
case class HttpRequestError(detail: String) extends ProgramError
case class CodeExecutionError(code: Int, message: String, error: Option[String]) extends ProgramError
