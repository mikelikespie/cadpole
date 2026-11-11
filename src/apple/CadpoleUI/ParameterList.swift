// `Model.params` as native controls: a Slider for a parameter that declares
// both min and max, else a TextField and a Stepper (as the web does), with the
// parameter's own step and its unit in the accessibility value ("2.4
// millimetres"), per docs/apple-apps.md. Which control, its range,
// its step and the printed number are CadpoleKit's ParameterControl.
//
// A slider drag runs: coalesced setParams -> re-evaluate -> re-render.

import CadpoleKit
import CadpoleProto
import SwiftUI

struct ParameterList: View {
    @Bindable var model: DocumentModel

    var body: some View {
        ForEach(model.model.params, id: \.name) { param in
            ParameterRow(model: model, param: param)
        }
    }
}

struct ParameterRow: View {
    @Bindable var model: DocumentModel
    let param: Cadpole_V1_ParamInfo

    @ScaledMetric(relativeTo: .body) private var fieldWidth: CGFloat = 72

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(param.name)
                    .font(.callout)
                if param.overridden {
                    Image(systemName: "pencil.circle.fill")
                        .foregroundStyle(.tint)
                        .help("Overridden; the script's default is \(param.defaultValue.displayString)")
                }
                Spacer()
                Text(valueText)
                    .font(.caption)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .frame(minWidth: fieldWidth, alignment: .trailing)
            }
            if param.value.isNumber {
                if let range = ParameterControl.sliderRange(param) {
                    Slider(value: binding, in: range, step: step) {
                        Text(param.name)
                    } minimumValueLabel: {
                        Text(ParameterControl.format(range.lowerBound)).font(.caption2)
                    } maximumValueLabel: {
                        Text(ParameterControl.format(range.upperBound)).font(.caption2)
                    }
                    .accessibilityValue(spokenValue)
                } else {
                    // No declared range: a field and a stepper, as the web
                    // does. A slider would have to invent a scale, and one
                    // derived from the live value ran away under the finger.
                    HStack {
                        TextField(param.name, value: binding, format: .number)
                            .textFieldStyle(.roundedBorder)
                            .monospacedDigit()
                            #if os(iOS)
                                .keyboardType(.numbersAndPunctuation)
                            #endif
                        Stepper(param.name, value: binding, step: step)
                            .labelsHidden()
                    }
                    .accessibilityValue(spokenValue)
                }
            }
            if !param.description_p.isEmpty {
                Text(param.description_p)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(param.name)
    }

    private var current: Double {
        model.paramValues[param.name] ?? param.value.number
    }

    private var binding: Binding<Double> {
        Binding(
            get: { current },
            set: { model.setParameter(param.name, to: $0) }
        )
    }

    private var step: Double {
        ParameterControl.step(param)
    }

    private var valueText: String {
        param.value.isNumber ? format(current) + unitSuffix : param.value.displayString
    }

    private var unitSuffix: String {
        param.unit.isEmpty ? "" : " " + param.unit
    }

    private var spokenValue: String {
        param.value.isNumber ? "\(format(current)) \(spokenUnit)" : param.value.displayString
    }

    /// The parameter's own declared unit, which is not the document's display
    /// unit and is not converted: `param(unit = "deg")` is degrees whatever
    /// the document displays lengths in.
    private var spokenUnit: String {
        UnitNames.spoken(param.unit)
    }

    private func format(_ value: Double) -> String {
        ParameterControl.format(value)
    }
}
