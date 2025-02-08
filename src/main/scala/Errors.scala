sealed trait ProgramError
case class AssistantMessageParseError(message: String) extends ProgramError
case class ArgumentParseError(argument: String) extends ProgramError
