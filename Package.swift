// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "Pickle",
    platforms: [.macOS(.v14)],
    products: [.library(name: "PickleCore", targets: ["PickleCore"]), .executable(name: "Pickle", targets: ["PickleApp"])],
    targets: [
        .target(name: "PickleCore"),
        .executableTarget(name: "PickleApp", dependencies: ["PickleCore"]),
        .executableTarget(name: "PickleChecks", dependencies: ["PickleCore"], path: "Tests/PickleCoreTests")
    ],
    swiftLanguageModes: [.v5]
)
