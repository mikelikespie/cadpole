# An L-profile with a filleted inner corner, written as a drawing.
#
# This is the shape that is miserable to type as coordinates: the fillet's
# centre is `(t + r, t + r)`, its ends are at `(t + r, t)` and `(t, t + r)`,
# and every one of those numbers has to be recomputed by hand when the
# thickness or the fillet radius changes. Said as constraints it is just "the
# arc is tangent to both inner faces and touches them where they end", and the
# solver does the arithmetic — sweep `fillet` from 2 to 10 and the corner
# follows.
#
# Units are millimetres.

leg = param("leg", 60, unit="mm", min=25, max=140, doc="Length of the foot (x)")
rise = param("rise", 45, unit="mm", min=25, max=140, doc="Height of the upright (y)")
t = param("thickness", 8, unit="mm", min=3, max=20)
r = param("fillet", 6, unit="mm", min=1, max=20, doc="Inner corner radius")
width = param("width", 20, unit="mm", min=5, max=60, doc="Extruded width (z)")

s = constrained_sketch(XZ, name="l_profile")

# Outline, counter-clockwise from the heel. The coordinates are the shape as
# the defaults happen to make it; the constraints are what keep it that shape.
heel = s.point(0, 0, name="heel", fix=True)
toe = s.point(leg, 0, name="toe")
toe_top = s.point(leg, t, name="toe_top")
fillet_bottom = s.point(t + r, t, name="fillet_bottom")
fillet_left = s.point(t, t + r, name="fillet_left")
tip_inner = s.point(t, rise, name="tip_inner")
tip_outer = s.point(0, rise, name="tip_outer")

sole = s.line(heel, toe, name="sole")
toe_face = s.line(toe, toe_top, name="toe_face")
foot_top = s.line(toe_top, fillet_bottom, name="foot_top")
web = s.line(fillet_left, tip_inner, name="web")
tip = s.line(tip_inner, tip_outer, name="tip")
spine = s.line(tip_outer, heel, name="spine")

# The fillet: a quarter arc whose centre is a point like any other, so the
# tangency constraints are free to move it.
corner = s.point(t + r, t + r, name="fillet_center")
fillet = s.arc(corner, r, start=180, end=270, name="fillet")

# Square everywhere except the fillet.
s.constrain("horizontal", sole)
s.constrain("horizontal", foot_top)
s.constrain("horizontal", tip)
s.constrain("vertical", toe_face)
s.constrain("vertical", web)
s.constrain("vertical", spine)

# The four dimensions a drawing would carry.
s.constrain("distance", heel, toe, leg, name="leg")
s.constrain("distance", heel, tip_outer, rise, name="rise")
s.constrain("distance", toe, toe_top, t, name="foot thickness")
s.constrain("distance", tip_inner, tip_outer, t, name="web thickness")

# The fillet meets both inner faces, and meets them tangentially. `tangent`
# with a third argument is the form for curves that already touch at a known
# point: the residual becomes "the tangents agree here", which is exactly what
# a fillet is.
s.constrain("coincident", fillet_bottom, fillet.end)
s.constrain("coincident", fillet_left, fillet.start)
s.constrain("tangent", foot_top, fillet, fillet_bottom, name="fillet meets the foot")
s.constrain("tangent", web, fillet, fillet_left, name="fillet meets the web")
s.constrain("radius", fillet, r, name="fillet radius")

assert_eq(s.dof(), 0)

angle = extrude(s, width, name="angle")
angle.material(preset="aluminum", roughness=0.35)

export(angle, "constrained_lprofile.stl")
