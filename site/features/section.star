# Section: cut a body with a plane and keep one side.
body = revolve(sketch(XZ).polygon(
    [(5, 0), (20, 0), (20, 6), (12, 6), (12, 24), (16, 24), (16, 30), (5, 30)]),
    name="body")
# snippet
cut = plane(origin=(0, 0, 0), normal=(0, -1, 0))
part = split(body, cut, keep="below", name="part")
# end snippet
part.material(color="#6fbfa8", roughness=0.4)
