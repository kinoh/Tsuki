package mumbleclient

import javax.net.ssl.*
import java.security.*
import java.nio.channels.SocketChannel
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.io.FileInputStream
import org.apache.hc.client5.http.utils.Hex
import javax.net.ssl.SSLEngineResult.HandshakeStatus
import javax.net.ssl.SSLEngineResult.Status
import java.security.cert.X509Certificate
import scala.util.{Try, Success, Failure}
import club.minnced.opus.util.OpusLibrary
import org.concentus.OpusDecoder
import org.apache.hc.core5.util.ByteArrayBuffer
import java.util.concurrent.BlockingQueue

type MumbleMessage =
  Mumble.Version
  // | Mumble.UDPTunnel
  | Mumble.Authenticate
  | Mumble.Ping
  | Mumble.Reject
  | Mumble.ServerSync
  | Mumble.ChannelRemove
  | Mumble.ChannelState
  | Mumble.UserRemove
  | Mumble.UserState
  | Mumble.BanList
  | Mumble.TextMessage
  | Mumble.PermissionDenied
  | Mumble.ACL
  | Mumble.QueryUsers
  | Mumble.CryptSetup
  | Mumble.ContextActionModify
  | Mumble.ContextAction
  | Mumble.UserList
  | Mumble.VoiceTarget
  | Mumble.PermissionQuery
  | Mumble.CodecVersion
  | Mumble.UserStats
  | Mumble.RequestBlob
  | Mumble.ServerConfig
  | Mumble.SuggestConfig
  | Mumble.PluginDataTransmission
  | MumbleUDP.Audio
  | MumbleUDP.Ping

enum MumblePacketType(val id: Char):
  case Version                extends MumblePacketType(0)
  case UDPTunnel              extends MumblePacketType(1)
  case Authenticate           extends MumblePacketType(2)
  case Ping                   extends MumblePacketType(3)
  case Reject                 extends MumblePacketType(4)
  case ServerSync             extends MumblePacketType(5)
  case ChannelRemove          extends MumblePacketType(6)
  case ChannelState           extends MumblePacketType(7)
  case UserRemove             extends MumblePacketType(8)
  case UserState              extends MumblePacketType(9)
  case BanList                extends MumblePacketType(10)
  case TextMessage            extends MumblePacketType(11)
  case PermissionDenied       extends MumblePacketType(12)
  case ACL                    extends MumblePacketType(13)
  case QueryUsers             extends MumblePacketType(14)
  case CryptSetup             extends MumblePacketType(15)
  case ContextActionModify    extends MumblePacketType(16)
  case ContextAction          extends MumblePacketType(17)
  case UserList               extends MumblePacketType(18)
  case VoiceTarget            extends MumblePacketType(19)
  case PermissionQuery        extends MumblePacketType(20)
  case CodecVersion           extends MumblePacketType(21)
  case UserStats              extends MumblePacketType(22)
  case RequestBlob            extends MumblePacketType(23)
  case ServerConfig           extends MumblePacketType(24)
  case SuggestConfig          extends MumblePacketType(25)
  case PluginDataTransmission extends MumblePacketType(26)

class MumbleClient(val hostname: String, val port: Int) {
  val engine = initEngine()
  val socketChannel = openSocketChannel()
  val session = engine.getSession()
  val myAppData = ByteBuffer.allocate(session.getApplicationBufferSize())
  val myNetData = ByteBuffer.allocate(session.getPacketBufferSize())
  val peerAppData = ByteBuffer.allocate(session.getApplicationBufferSize())
  val peerNetData = ByteBuffer.allocate(session.getPacketBufferSize())

  def audioChannels: Int =
    1
  def audioBufferSize: Int =
    960 * audioChannels

  private def openSocketChannel(): SocketChannel =
    val channel = SocketChannel.open()
    channel.configureBlocking(false)
    channel.connect(InetSocketAddress(hostname, port))
    while (!channel.finishConnect()) {
      Thread.sleep(100)
    }
    channel

  private def initEngine(): SSLEngine =
    // https://stackoverflow.com/q/52988677
    val trustAllCerts = Array[TrustManager](new X509TrustManager {
      override def getAcceptedIssuers(): Array[X509Certificate] = null
      override def checkClientTrusted(certs: Array[X509Certificate], authType: String): Unit = {}
      override def checkServerTrusted(certs: Array[X509Certificate], authType: String): Unit = {}
    })

    val sslContext = SSLContext.getInstance("TLSv1.2")
    sslContext.init(null, trustAllCerts, null)
    val engine = sslContext.createSSLEngine(hostname, port)
    engine.setUseClientMode(true)
    engine

  private def doHandshake(): Option[String] =
    val appBufferSize = engine.getSession().getApplicationBufferSize()
    val myAppData = ByteBuffer.allocate(appBufferSize)
    val peerAppData = ByteBuffer.allocate(appBufferSize)

    engine.beginHandshake()
    var hs = engine.getHandshakeStatus()
    while (true) {
      println(s"HandshakeStatus: ${hs}")
      hs match {
        case HandshakeStatus.NEED_UNWRAP =>
          var remaining = true
          while (remaining) {
            if (socketChannel.read(peerNetData) < 0) {
              return Some("channel closed")
            }
            peerNetData.flip()
            val res = engine.unwrap(peerNetData, peerAppData)
            peerNetData.compact()
            hs = res.getHandshakeStatus()
            res.getStatus() match {
              case Status.OK =>
                remaining = false
              case Status.BUFFER_UNDERFLOW =>
                Thread.sleep(100)
              case Status.BUFFER_OVERFLOW =>
                return Some("buffer overflow")
              case Status.CLOSED =>
                return Some("closed")
            }
          }
        case HandshakeStatus.NEED_WRAP =>
            myNetData.clear()
            val res = engine.wrap(myAppData, myNetData)
            hs = res.getHandshakeStatus()
            res.getStatus() match {
              case Status.OK =>
                myNetData.flip()
                while (myNetData.hasRemaining()) {
                  if (socketChannel.write(myNetData) < 0) {
                    return Some("channel closed")
                  }
                }
              case Status.BUFFER_OVERFLOW =>
                return Some("buffer overflow")
              case Status.BUFFER_UNDERFLOW =>
                return Some("buffer underflow")
              case Status.CLOSED =>
                return Some("closed")
            }
        case HandshakeStatus.NEED_TASK =>
          var task: Runnable = null
          val getTask = () =>
            task = engine.getDelegatedTask()
            task
          while (getTask() != null) {
            task.run()
          }
          hs = engine.getHandshakeStatus()
          if (hs == HandshakeStatus.NEED_TASK) {
            return Some("handshake shouldn't need additional tasks")
          }

        case HandshakeStatus.FINISHED =>
          return None
        case HandshakeStatus.NEED_UNWRAP_AGAIN =>
          return Some("need unwrap again (not implemented)")
        case HandshakeStatus.NOT_HANDSHAKING =>
          return Some("not handshaking")
      }
    }
    Some("unreachable")

  def connect(sharedBuffer: Array[Short], receivedSizeNotifier: BlockingQueue[Int]): Option[String] =
    doHandshake() match {
      case None =>
      case Some(err) =>
        return Some(s"handshake error: ${err}")
    }

    println("receive version message")

    readMessage() match {
      case Left("no data") =>
        return Some("version not received")
      case Right(message: Mumble.Version) =>
        println("Version:")
        println(s"  versionV1: ${message.versionV1}")
        println(s"  versionV2: ${message.versionV2}")
        println(s"  os: ${message.os}")
        println(s"  osVersion: ${message.osVersion}")
        println(s"  release: ${message.release}")
      case Right(message) =>
        return Some("unexpected packet")
      case Left(err) =>
        return Some(err)
    }

    println("send version message")

    val version = Mumble.Version(
      None,
      Some(0x0001000500000000L),
      Some("Tsuki"),
      Option(System.getProperty("os.name")),
      Option(System.getProperty("os.version"))
    )
    sendMessage(version)

    println("send auth")

    val auth = Mumble.Authenticate(
      Some("tsuki"),
      None
    )
    sendMessage(auth)

    println("receive CryptSetup")

    val decoder = OpusDecoder(48000, audioChannels)

    if sharedBuffer.length < audioBufferSize then
      return Some("buffer length not enough")

    while (true) {
      val num = socketChannel.read(peerNetData)
      if (num == -1) {
        return Some("closed!")
      }
      else if (num == 0) {
        Thread.sleep(2)
      } else {
        readMessage() match {
          case Left("no data") =>
            println("no data")
          case Right(message: Mumble.CryptSetup) =>
            println("CryptSetup:")
            println(s"  clientNonce: ${message.clientNonce}")
            println(s"  serverNonce: ${message.serverNonce}")
            println(s"  key: ${message.key}")
          case Right(audio: MumbleUDP.Audio) =>
            println(s"Audio: header=${audio.header} session=${audio.senderSession} framenum=${audio.frameNumber}")
            val data = audio.opusData.toByteArray()
            val n = sharedBuffer.synchronized {
              decoder.decode(data, 0, data.length, sharedBuffer, 0, sharedBuffer.length, false)
            }
            receivedSizeNotifier.put(n)
          case Right(message) =>
            println(s"unexpected packet: ${message}")
          case Left(err) =>
            return Some(err)
        }
      }
    }
    Some("unreachable code")

  private def sendMessage(message: MumbleMessage): Option[String] =
    val packetType = message match {
      case _: Mumble.Version                => MumblePacketType.Version
      // case _: Mumble.UDPTunnel              => MumblePacketType.UDPTunnel
      case _: Mumble.Authenticate           => MumblePacketType.Authenticate
      case _: Mumble.Ping                   => MumblePacketType.Ping
      case _: Mumble.Reject                 => MumblePacketType.Reject
      case _: Mumble.ServerSync             => MumblePacketType.ServerSync
      case _: Mumble.ChannelRemove          => MumblePacketType.ChannelRemove
      case _: Mumble.ChannelState           => MumblePacketType.ChannelState
      case _: Mumble.UserRemove             => MumblePacketType.UserRemove
      case _: Mumble.UserState              => MumblePacketType.UserState
      case _: Mumble.BanList                => MumblePacketType.BanList
      case _: Mumble.TextMessage            => MumblePacketType.TextMessage
      case _: Mumble.PermissionDenied       => MumblePacketType.PermissionDenied
      case _: Mumble.ACL                    => MumblePacketType.ACL
      case _: Mumble.QueryUsers             => MumblePacketType.QueryUsers
      case _: Mumble.CryptSetup             => MumblePacketType.CryptSetup
      case _: Mumble.ContextActionModify    => MumblePacketType.ContextActionModify
      case _: Mumble.ContextAction          => MumblePacketType.ContextAction
      case _: Mumble.UserList               => MumblePacketType.UserList
      case _: Mumble.VoiceTarget            => MumblePacketType.VoiceTarget
      case _: Mumble.PermissionQuery        => MumblePacketType.PermissionQuery
      case _: Mumble.CodecVersion           => MumblePacketType.CodecVersion
      case _: Mumble.UserStats              => MumblePacketType.UserStats
      case _: Mumble.RequestBlob            => MumblePacketType.RequestBlob
      case _: Mumble.ServerConfig           => MumblePacketType.ServerConfig
      case _: Mumble.SuggestConfig          => MumblePacketType.SuggestConfig
      case _: Mumble.PluginDataTransmission => MumblePacketType.PluginDataTransmission
      case _: MumbleUDP.Audio               => MumblePacketType.UDPTunnel
      case _: MumbleUDP.Ping                => MumblePacketType.UDPTunnel
    }
    val payload = message match {
      case pb: scalapb.GeneratedMessage =>
        pb.toByteArray
    }

    myAppData.clear()
    myAppData.putChar(packetType.id)
    myAppData.putInt(payload.length)
    myAppData.put(payload)
    myAppData.flip()

    while (myAppData.hasRemaining()) {
      myNetData.clear()
      val res = engine.wrap(myAppData, myNetData)
      res.getStatus() match {
        case Status.OK =>
          myNetData.flip()
          while (myNetData.hasRemaining()) {
            val num = socketChannel.write(myNetData)
            println(s"written: ${num}")
            if (num == -1) {
              return Some("closed channel")
            } else if (num == 0) {
              // no bytes written; try again later
              Thread.sleep(100)
            }
          }
        case Status.BUFFER_UNDERFLOW =>
          return Some("BUFFER_UNDERFLOW")
        case Status.BUFFER_OVERFLOW =>
          return Some("BUFFER_OVERFLOW")
        case Status.CLOSED =>
          return Some("CLOSED")
      }
    }
    None

  private def readMessage(): Either[String, MumbleMessage] =
    peerNetData.flip()
    if peerNetData.hasRemaining() then
      peerAppData.clear()
      val res = engine.unwrap(peerNetData, peerAppData)
      res.getStatus() match {
        case Status.OK =>
          peerNetData.compact()
          peerAppData.flip()
          if peerAppData.hasRemaining() then
            parseMessage(peerAppData)
          else
            Left("no unwrapped data")
        case Status.BUFFER_OVERFLOW =>
          Left("BUFFER_OVERFLOW")
        case Status.BUFFER_UNDERFLOW =>
          Left("BUFFER_UNDERFLOW")
        case Status.CLOSED =>
          Left("CLOSED")
      }
    else
      Left("no data")

  private def parseMessage(packet: ByteBuffer): Either[String, MumbleMessage] =
    val packetType = packet.getChar()
    val payloadLength = packet.getInt()
    val buffer = new Array[Byte](payloadLength)
    packet.get(buffer, 0, payloadLength)
    println(s"packetType=${packetType.toInt} payloadLength=${payloadLength}")
    // println(s"payload: ${Hex.encodeHexString(buffer)}")
    val data = packetType match {
      case MumblePacketType.Version.id =>                Mumble.Version.validate(buffer)
      case MumblePacketType.Authenticate.id =>           Mumble.Authenticate.validate(buffer)
      case MumblePacketType.Ping.id =>                   Mumble.Ping.validate(buffer)
      case MumblePacketType.Reject.id =>                 Mumble.Reject.validate(buffer)
      case MumblePacketType.ServerSync.id =>             Mumble.ServerSync.validate(buffer)
      case MumblePacketType.ChannelRemove.id =>          Mumble.ChannelRemove.validate(buffer)
      case MumblePacketType.ChannelState.id =>           Mumble.ChannelState.validate(buffer)
      case MumblePacketType.UserRemove.id =>             Mumble.UserRemove.validate(buffer)
      case MumblePacketType.UserState.id =>              Mumble.UserState.validate(buffer)
      case MumblePacketType.BanList.id =>                Mumble.BanList.validate(buffer)
      case MumblePacketType.TextMessage.id =>            Mumble.TextMessage.validate(buffer)
      case MumblePacketType.PermissionDenied.id =>       Mumble.PermissionDenied.validate(buffer)
      case MumblePacketType.ACL.id =>                    Mumble.ACL.validate(buffer)
      case MumblePacketType.QueryUsers.id =>             Mumble.QueryUsers.validate(buffer)
      case MumblePacketType.CryptSetup.id =>             Mumble.CryptSetup.validate(buffer)
      case MumblePacketType.ContextActionModify.id =>    Mumble.ContextActionModify.validate(buffer)
      case MumblePacketType.ContextAction.id =>          Mumble.ContextAction.validate(buffer)
      case MumblePacketType.UserList.id =>               Mumble.UserList.validate(buffer)
      case MumblePacketType.VoiceTarget.id =>            Mumble.VoiceTarget.validate(buffer)
      case MumblePacketType.PermissionQuery.id =>        Mumble.PermissionQuery.validate(buffer)
      case MumblePacketType.CodecVersion.id =>           Mumble.CodecVersion.validate(buffer)
      case MumblePacketType.UserStats.id =>              Mumble.UserStats.validate(buffer)
      case MumblePacketType.RequestBlob.id =>            Mumble.RequestBlob.validate(buffer)
      case MumblePacketType.ServerConfig.id =>           Mumble.ServerConfig.validate(buffer)
      case MumblePacketType.SuggestConfig.id =>          Mumble.SuggestConfig.validate(buffer)
      case MumblePacketType.PluginDataTransmission.id => Mumble.PluginDataTransmission.validate(buffer)
      case MumblePacketType.UDPTunnel.id =>
        buffer(0) match {
          case 0x00 => MumbleUDP.Audio.validate(buffer.slice(1, buffer.length))
          case 0x20 => MumbleUDP.Ping.validate(buffer.slice(1, buffer.length))
          case _    => Failure(Throwable("unexpected UDP packet header"))
        }
    }
    data.toEither.left.map(t => t.toString())
}
