import java.util as ju
import java.util.concurrent.BlockingQueue
import java.util.concurrent.TimeUnit

class MappedBlockingQueue[E, F](queue: BlockingQueue[E], f: E => F) extends BlockingQueue[F] {
  // Members declared in java.util.Collection
  def addAll(x$0: java.util.Collection[? <: F]): Boolean = ???
  def clear(): Unit = ???
  def containsAll(x$0: java.util.Collection[?]): Boolean = ???
  def isEmpty(): Boolean = ???
  def iterator(): java.util.Iterator[F] = ???
  def removeAll(x$0: java.util.Collection[?]): Boolean = ???
  def retainAll(x$0: java.util.Collection[?]): Boolean = ???
  def size(): Int = ???
  def toArray(): Array[Object] = ???
  def toArray[T](x$0: Array[Object & T]): Array[Object & T] = ???
  
  // Members declared in java.util.Queue
  def element(): F = ???
  def peek(): F = ???
  def poll(): F = ???
  def remove(): F = ???

  def add(e: F): Boolean = ???
  def contains(o: Object): Boolean = ???
  def drainTo(c: ju.Collection[? >: F]): Int = ???
  def drainTo(c: ju.Collection[? >: F], maxElements: Int): Int = ???
  def offer(e: F): Boolean = ???
  def offer(e: F, timeout: Long, unit: TimeUnit): Boolean = ???
  def put(e: F): Unit = ???
  def remove(o: Object): Boolean = ???

  def poll(timeout: Long, unit: TimeUnit): F = f(queue.poll(timeout, unit))
  def remainingCapacity(): Int = queue.remainingCapacity() 
  def take(): F = f(queue.take())
}

object MappedBlockingQueue {
  def apply[E, F](queue: BlockingQueue[E], f: E => F): MappedBlockingQueue[E, F] =
    new MappedBlockingQueue(queue, f)
}
