# Parameters: named numbers the model is rebuilt from.
# snippet
w = param("width", 40, unit="mm", min=20, max=80)
holes = param("holes", 3, min=1, max=6)
plate = sketch(XY).rounded_rect(w, 20, 4)
for i in range(holes):
    x = (i + 0.5) * w / holes - w / 2
    plate = plate.circle(3, center=(x, 0))
# end snippet
part = extrude(plate, 6, name="part")
part.material(color="#6fbfa8", roughness=0.4)
