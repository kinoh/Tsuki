val scala3Version = "3.6.3"

enablePlugins(JavaAppPackaging)
enablePlugins(DockerPlugin)

dockerBaseImage := "eclipse-temurin:latest"

lazy val root = project
  .in(file("."))
  .settings(
    name := "tsuki",
    version := "0.1.0-SNAPSHOT",

    scalaVersion := scala3Version,
  
    resolvers += "jitpack" at "https://jitpack.io",

    libraryDependencies += "org.scalameta" %% "munit" % "1.0.0" % Test,
    libraryDependencies += "net.dv8tion" % "JDA" % "5.0.0-beta.15",
    libraryDependencies += "com.openai" % "openai-java" % "0.21.1",
    libraryDependencies += "com.lihaoyi" %% "upickle" % "4.1.0",
    libraryDependencies += "com.softwaremill.sttp.client4" %% "core" % "4.0.0-RC1",
    libraryDependencies += "net.java.dev.jna" % "jna" % "5.16.0",
    libraryDependencies += "com.alphacephei" % "vosk" % "0.3.45",
    libraryDependencies += "com.github.lostromb.concentus" % "Concentus" % "6c2328dc19044601e33a9c11628b8d60e1f3011c",
    libraryDependencies += "com.outr" %% "scribe" % "3.16.0",
  )

Compile / PB.targets := Seq(
  scalapb.gen(flatPackage=true) -> (Compile / sourceManaged).value / "scalapb"
)
