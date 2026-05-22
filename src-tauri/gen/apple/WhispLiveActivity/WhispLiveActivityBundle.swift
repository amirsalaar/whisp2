#if os(iOS)
import SwiftUI
import WidgetKit

@main
struct WhispLiveActivityBundle: WidgetBundle {
    var body: some Widget {
        if #available(iOS 17.0, *) {
            WhispLiveActivityWidget()
        }
    }
}
#endif
