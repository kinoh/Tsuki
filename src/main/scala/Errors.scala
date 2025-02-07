sealed trait ProgramError
case class AssistantMessageParseError(message: String) extends ProgramError
