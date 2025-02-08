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

    libraryDependencies += "org.scalameta" %% "munit" % "1.0.0" % Test,
    libraryDependencies += "net.dv8tion" % "JDA" % "5.0.0-beta.15",
    libraryDependencies += "com.openai" % "openai-java" % "0.21.1",
    libraryDependencies += "com.lihaoyi" %% "upickle" % "4.1.0",
  )
