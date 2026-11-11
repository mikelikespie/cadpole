// Nothing here may be flagged.
module precedence_good {
  var x: bool
  var n: int
  val ready = true
  // A conjunction of assignments: the usual style.
  action g1 = x' = true and n' = n
  action g2 = x' = (ready and n > 0)
  action g3 = (x' = ready) and n > 0
  action g4 = all {
    x' = Set(1, 2).exists(v => v > 0 and ready),
    n' = n,
  }
  action g5 = x' = ready.and(true)   // a method call
  // x' = ready and n > 0   (a comment)
  action g6 = all { x' = ready, n' = n } and n >= 0
  val s = "x' = a and b"
  action g7 = x' = ready
    or n' = n + 1
}
