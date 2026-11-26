// Make an app the default handler for files with a given extension (no duti needed).
//   usage: swift macos-set-default.swift <extension> <bundle id>
import CoreServices
import Foundation
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 3, let type = UTType(filenameExtension: args[1]) else {
    FileHandle.standardError.write("usage: macos-set-default.swift <extension> <bundle id>\n".data(using: .utf8)!)
    exit(2)
}
let status = LSSetDefaultRoleHandlerForContentType(type.identifier as CFString, .all, args[2] as CFString)
if status != noErr {
    FileHandle.standardError.write("LSSetDefaultRoleHandlerForContentType failed: \(status)\n".data(using: .utf8)!)
    exit(1)
}
print(".\(args[1]) (\(type.identifier)) now opens with \(args[2])")
