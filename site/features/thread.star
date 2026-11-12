# Threads: a real helical thread from the standard library.
# snippet
load("@cadpole/mechanical.star", "bolt", "nut")
screw = bolt(12, 1.75, 40, 18, 7.5, name="screw")  # M12 x 1.75
ring = nut(12, 1.75, 18, 10, start=(0, 0, 12),
           name="ring")
# end snippet
screw.material(color="#6fbfa8", roughness=0.35)
ring.material(color="#f0c987", roughness=0.35)


def the_nut_runs_free_on_the_thread():
    assert_solid(screw)
    assert_solid(ring)
    gap = distance(screw, ring)
    assert_true(not gap.overlap, msg="the nut does not cut into the bolt")
    assert_true(gap.distance > 0.02, msg="the nut clears the bolt's thread")


def the_head_nut_and_tip_are_chamfered():
    # A bare hex keeps its whole area up to its faces; the 30 degree
    # chamfer turns the corners off near the head's top face and both
    # faces of the nut, and the tip is chamfered to the thread's root.
    def area(body, z):
        return body.section(plane(origin=(0, 0, z), normal=(0, 0, 1))).area()

    assert_true(area(screw, 47.4) < 0.9 * area(screw, 44), msg="the head's top corners are turned off")
    assert_true(area(ring, 12.1) < 0.9 * area(ring, 17), msg="the nut's lower corners are turned off")
    assert_true(area(ring, 21.9) < 0.9 * area(ring, 17), msg="the nut's upper corners are turned off")
    assert_true(area(screw, 0.05) < math.pi * 5.1 * 5.1, msg="the tip is chamfered to the minor diameter")


test("the nut runs free on the bolt's thread", the_nut_runs_free_on_the_thread)
test("the head, the nut and the tip are chamfered", the_head_nut_and_tip_are_chamfered)
