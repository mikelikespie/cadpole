// Every assignment here parses as `(x' = ...) and/or ...`: the lint must flag each.
module precedence_bad {
  var x: bool
  var n: int
  val ready = true
  action a1 = x' = ready and n > 0
  action a2 = all {
    x' = x or ready,
    n' = n,
  }
  action a3 = all {
    n' = n + 1,
    x' = x
      and ready,
  }
  action a4 = x' = if (ready) true else false and n > 0
}
