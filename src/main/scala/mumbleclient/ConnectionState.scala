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
        println(s"received: ${x}")
        receive(rest)
      case (x: Mumble.CryptSetup) :: rest =>
        println(s"received: ${x}")
        receive(rest)
      case (x: Mumble.CodecVersion) :: rest =>
        println(s"received: ${x}")
        receive(rest)
      case (x: Mumble.ChannelState) :: rest =>
        println(s"received: ${x}")
        (x.channelId, x.name) match
          case (Some(channelId), Some(name)) =>
            addChannel(channelId -> name).receive(rest)
          case _ =>
            Left("incomplete ChannelState")
      case (x: Mumble.PermissionQuery) :: rest =>
        println(s"received: ${x}")
        receive(rest)
      case (x: Mumble.UserState) :: rest =>
        println(s"received: ${x}")
        (x.session, x.name) match
          case (Some(session), Some(name)) =>
            addUser(session -> name).receive(rest)
          case _ =>
            Left("incomplete UserState")
      case (x: Mumble.ServerSync) :: rest =>
        println(s"received: ${x}")
        receive(rest)
      case (x: Mumble.ServerConfig) :: rest =>
        println(s"received: ${x}")
        receive(rest)
      case _ :: rest =>
        Left("unexpected message")
      case Nil =>
        Right(this)
}
