// What a parameter row shows, without SwiftUI: the number it prints and the
// control it gets. CadpoleUI's ParameterRow draws these; they live here so the
// Linux tests pin them.

import Foundation
import CadpoleProto

public enum ParameterControl {
    /// A parameter value as the row prints it (label, VoiceOver, the slider's
    /// end labels): `UnitNames.trimmed`, the web's `fmt`, so it is the value
    /// the model is evaluated with to three decimals -- not three significant
    /// digits, which printed 152.4 as "152" and 1234.5 as "1.23e+03".
    public static func format(_ value: Double) -> String {
        UnitNames.trimmed(value)
    }

    /// The slider's range, or nil for no slider. Only a parameter with both
    /// bounds declared gets one, as on the web (src/web/src/ui/params.ts):
    /// anything else has no honest scale. The range used to be derived from
    /// the live value, so it doubled under the finger on every drag frame and
    /// gave -5 the range -10...-9, which does not contain it.
    public static func sliderRange(_ param: Cadpole_V1_ParamInfo) -> ClosedRange<Double>? {
        guard param.hasMin, param.hasMax, param.min.isFinite, param.max.isFinite, param.max > param.min else {
            return nil
        }
        return param.min ... param.max
    }

    /// One slider detent or stepper tap: the declared step, else the web's
    /// `niceStep` -- a 1-2-5 step near a hundredth of the range, or 1 when
    /// there is no range.
    public static func step(_ param: Cadpole_V1_ParamInfo) -> Double {
        if param.hasStep, param.step > 0, param.step.isFinite { return param.step }
        guard let range = sliderRange(param) else { return 1 }
        let raw = (range.upperBound - range.lowerBound) / 100
        let pow = Foundation.pow(10, (log10(raw)).rounded(.down))
        let r = raw / pow
        return r >= 5 ? pow * 5 : r >= 2 ? pow * 2 : pow
    }
}
