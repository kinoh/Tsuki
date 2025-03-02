package mumbleclient

import java.nio.channels.SocketChannel

class ConnectionState(val socketChannel: SocketChannel, val channelById: Map[Int, String], val userBySession: Map[Int, String]) {

  private def addUser(user: (Int, String)): ConnectionState =
    ConnectionState(socketChannel, channelById, userBySession + user)

  private def addChannel(channel: (Int, String)): ConnectionState =
    ConnectionState(socketChannel, channelById + channel, userBySession)

  def receive(messages: Seq[MumbleMessage]): Either[String, ConnectionState] =
    messages match
      case (x: Mumble.Version) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case (x: Mumble.CryptSetup) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case (x: Mumble.CodecVersion) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case (x: Mumble.ChannelState) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        (x.channelId, x.name) match
          case (Some(channelId), Some(name)) =>
            addChannel(channelId -> name).receive(rest)
          case _ =>
            Left("incomplete ChannelState")
      case (x: Mumble.PermissionQuery) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case (x: Mumble.UserState) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        (x.session, x.name) match
          case (Some(session), Some(name)) =>
            addUser(session -> name).receive(rest)
          case _ =>
            Left("incomplete UserState")
      case (x: Mumble.ServerSync) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case (x: Mumble.ServerConfig) :: rest =>
        scribe.debug("accept", scribe.data("message", x))
        receive(rest)
      case _ :: rest =>
        Left("unexpected message")
      case Nil =>
        Right(this)
}
