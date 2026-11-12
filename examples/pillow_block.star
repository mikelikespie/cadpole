# A pillow block (a bearing housing), designed in inches: 4 x 1 1/4 x 2.
# One cast profile: a flat foot, a web whose flanks run tangent into the
# barrel, and fillets where the web meets the foot. The bore is chamfered at
# both ends, the foot's ends are rounded in plan, and the foot is slotted
# so the block can be squared up on its bolts.
inch = 25.4
length = param("length", 4.0, unit="in", min=3.5, max=6.0)
bore_d = param("bore", 0.75, unit="in", min=0.375, max=1.0)

centre_h = 1.375      # bore axis above the underside
barrel_r = 0.625
depth = 1.25          # front to back
web_root = 1.0        # half the web's width where it meets the foot
foot_t = 0.5
root_r = 0.125        # the fillet where a flank meets the foot
slot_l, slot_w = 0.5, 0.28
slot_x = length / 2 - 0.5

def flank_tangent():
    # Where the right flank, from its root (web_root, foot_t), touches the
    # barrel's circle about (0, centre_h). math works in degrees.
    px, pz = web_root, foot_t - centre_h
    t = math.atan2(pz, px) + math.acos(barrel_r / math.hypot(px, pz))
    return (barrel_r * math.cos(t), centre_h + barrel_r * math.sin(t))

def root_fillet(tx, tz):
    # The fillet in the corner between the foot's top, running out along +x,
    # and the right flank, rising to (tx, tz): its two tangent points and the
    # middle of its arc.
    ux, uz = tx - web_root, tz - foot_t
    n = math.hypot(ux, uz)
    ux, uz = ux / n, uz / n
    wedge = math.atan2(uz, ux)              # the open angle at the corner, degrees
    d = root_r / math.tan(wedge / 2)        # corner to each tangent point
    reach = root_r / math.sin(wedge / 2)    # corner to the fillet's centre
    bx, bz = math.cos(wedge / 2), math.sin(wedge / 2)
    on_foot = (web_root + d, foot_t)
    on_flank = (web_root + d * ux, foot_t + d * uz)
    mid = (web_root + (reach - root_r) * bx, foot_t + (reach - root_r) * bz)
    return on_foot, mid, on_flank

tx, tz = flank_tangent()
f0, fm, f1 = root_fillet(tx, tz)
half = length / 2

def mm(p, side=1):
    return (side * p[0] * inch, p[1] * inch)

# The front profile, from the underside's left end round over the barrel.
outline = (path(mm((-half, 0))).line(mm((half, 0))).line(mm((half, foot_t)))
           .line(mm(f0)).arc(mm(f1), via=mm(fm))
           .line(mm((tx, tz))).arc(mm((tx, tz), -1), via=mm((0, centre_h + barrel_r)))
           .line(mm(f1, -1)).arc(mm(f0, -1), via=mm(fm, -1))
           .line(mm((-half, foot_t))).line(mm((-half, 0))))
profile = sketch(XZ).add(outline)
housing = translate(extrude(profile, depth * inch), (0, depth * inch / 2, 0), name="housing")
# The foot's four ends rounded in plan.
ends = housing.edges(direction=(0, 0, 1), min_length=0.4 * inch)
housing = fillet(housing, 0.25 * inch, edges=ends, name="cast")

# The bore and its two 45-degree chamfers as one tool: a chamfer cut into
# the mouth of an existing hole is a curve enclosing an inner loop, which the
# exact kernel does not split a face along. Each cone starts inside the bore
# and crosses its wall where the chamfer begins.
chamfer_c = 0.04
def bore_tool():
    r, c, e, dip = bore_d / 2 * inch, chamfer_c * inch, 2.0, 0.5
    z = depth / 2 * inch
    run = c + e + dip
    parts = [cylinder(r, 2 * z, at=(0, 0, -z)),
             cone(r - dip, r + c + e, run, at=(0, 0, z - c - dip)),
             rotate(cone(r - dip, r + c + e, run, at=(0, 0, z - c - dip)), 180, axis=(1, 0, 0))]
    tool = union(parts[0], parts[1], parts[2], kernel="brep")
    return translate(rotate(tool, 90, axis=(1, 0, 0)), (0, 0, centre_h * inch), name="bore")

bore = bore_tool()
slots = extrude(sketch(XY.offset(-0.1 * inch))
                .slot(slot_l * inch, slot_w * inch, center=(-slot_x * inch, 0))
                .slot(slot_l * inch, slot_w * inch, center=(slot_x * inch, 0)),
                (foot_t + 0.2) * inch, name="slots")

block = subtract(housing, [bore] + slots, kernel="brep", name="block")
block.material(color="#8fb7c9", roughness=0.45, metallic=0.35)


# Tests: `cadpole test examples/pillow_block.star`.

def the_block_is_one_solid():
    assert_solid(block)
    lo, hi = block.bbox()
    assert_true(abs(hi[0] - lo[0] - length * inch) < 1e-3, msg="the foot is `length` long")
    assert_true(abs(hi[2] - lo[2] - 2 * inch) < 0.05, msg="the barrel's crown is 2 in above the foot's underside")
    return block.volume()

test("one solid, 2 in tall, as long as `length`", the_block_is_one_solid, sweep={"length": [3.5, 4.0, 6.0]}, monotone=monotone.increasing)
test("a wider bore leaves less block", the_block_is_one_solid, sweep={"bore": [0.375, 0.75, 1.0]}, monotone=monotone.decreasing)

def the_casting_is_finished():
    assert_true(block.section(XY.offset(foot_t * inch / 2)).loops == 3, msg="the foot, slotted twice")
    assert_true(block.section(XY.offset(centre_h * inch)).loops == 2, msg="the bore runs right through the barrel")
    lo, hi = block.bbox()
    assert_true(abs(hi[1] - lo[1] - depth * inch) < 1e-3, msg="foot, web and barrel are one depth: nothing stands proud")
    assert_true(len(block.faces(name="*blend*")) == 4, msg="the foot's four ends are rounded in plan")
    chamfers = block.faces(kind="cone")
    assert_true(len(chamfers) >= 2, msg="both ends of the bore are chamfered, got %d cone faces" % len(chamfers))
    roots = block.faces(kind="cylinder", inside=((-web_root * inch - 4, -depth * inch, foot_t * inch - 0.01),
                                                  (web_root * inch + 4, depth * inch, (foot_t + root_r) * inch + 0.01)))
    assert_true(len(roots) == 2, msg="a fillet at each flank's root, got %d" % len(roots))
    return True

test("the casting has its slots, root fillets and chamfered bore", the_casting_is_finished, sweep={"length": [3.5, 6.0], "bore": [0.375, 1.0]})
