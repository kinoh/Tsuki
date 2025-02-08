sealed trait ProgramError
case class MessageParseError(message: String) extends ProgramError
case class ArgumentParseError(argument: String) extends ProgramError
case class AssertionError(detail: String) extends ProgramError
