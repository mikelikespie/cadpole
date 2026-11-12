import Compression
import Foundation

/// A read-only ZIP reader for 3MF packages: the central directory, stored
/// and deflated entries, and the zip64 fields slicers write for large parts.
/// Only the entries asked for are decompressed; the archive is memory-mapped,
/// so the mesh parts a preview never reads are never paged in.
public struct ZipReader: Sendable {
    public struct Entry: Sendable {
        public let name: String
        let method: UInt16
        let compressedSize: Int
        let uncompressedSize: Int
        let localHeaderOffset: Int
    }

    public enum Failure: Error, Equatable {
        case notAZip
        case truncated(String)
        case unsupportedMethod(String, UInt16)
        case inflateFailed(String)
    }

    private let data: Data
    public let entries: [String: Entry]

    public init(url: URL) throws {
        try self.init(data: Data(contentsOf: url, options: .alwaysMapped))
    }

    public init(data: Data) throws {
        self.data = data
        self.entries = try Self.readCentralDirectory(data)
    }

    public func contains(_ name: String) -> Bool { entries[name] != nil }

    /// The entry's bytes, or nil when the archive has no such entry.
    public func read(_ name: String) throws -> Data? {
        guard let entry = entries[name] else { return nil }
        let header = entry.localHeaderOffset
        guard header + 30 <= data.count, data.u32(header) == 0x0403_4B50 else { throw Failure.truncated(name) }
        let start = header + 30 + Int(data.u16(header + 26)) + Int(data.u16(header + 28))
        guard start + entry.compressedSize <= data.count else { throw Failure.truncated(name) }
        let raw = data.subdata(in: data.startIndex + start ..< data.startIndex + start + entry.compressedSize)
        switch entry.method {
        case 0:
            return raw
        case 8:
            return try Self.inflate(raw, size: entry.uncompressedSize, name: name)
        default:
            throw Failure.unsupportedMethod(name, entry.method)
        }
    }

    /// Raw DEFLATE (RFC 1951): what COMPRESSION_ZLIB decodes.
    private static func inflate(_ raw: Data, size: Int, name: String) throws -> Data {
        if size == 0 { return Data() }
        var out = Data(count: size)
        let written = out.withUnsafeMutableBytes { (dst: UnsafeMutableRawBufferPointer) -> Int in
            raw.withUnsafeBytes { (src: UnsafeRawBufferPointer) -> Int in
                compression_decode_buffer(
                    dst.bindMemory(to: UInt8.self).baseAddress!, size,
                    src.bindMemory(to: UInt8.self).baseAddress!, raw.count,
                    nil, COMPRESSION_ZLIB)
            }
        }
        guard written == size else { throw Failure.inflateFailed(name) }
        return out
    }

    private static func readCentralDirectory(_ data: Data) throws -> [String: Entry] {
        // The end-of-central-directory record is 22 bytes plus a comment of
        // at most 65535 bytes, so it is somewhere in the last 65557 bytes.
        guard data.count >= 22 else { throw Failure.notAZip }
        var eocd = -1
        var i = data.count - 22
        let floor = max(0, data.count - 65557)
        while i >= floor {
            if data.u32(i) == 0x0605_4B50 { eocd = i; break }
            i -= 1
        }
        guard eocd >= 0 else { throw Failure.notAZip }

        var count = Int(data.u16(eocd + 10))
        var cdSize = Int(data.u32(eocd + 12))
        var cdOffset = Int(data.u32(eocd + 16))
        // A zip64 locator sits immediately before the classic record.
        if eocd >= 20, data.u32(eocd - 20) == 0x0706_4B50 {
            let z64 = Int(data.u64(eocd - 20 + 8))
            guard z64 + 56 <= data.count, data.u32(z64) == 0x0606_4B50 else { throw Failure.truncated("zip64 end record") }
            count = Int(data.u64(z64 + 32))
            cdSize = Int(data.u64(z64 + 40))
            cdOffset = Int(data.u64(z64 + 48))
        }
        guard cdOffset >= 0, cdSize >= 0, cdOffset + cdSize <= data.count else { throw Failure.truncated("central directory") }

        var entries: [String: Entry] = [:]
        entries.reserveCapacity(count)
        var p = cdOffset
        for _ in 0 ..< count {
            guard p + 46 <= data.count, data.u32(p) == 0x0201_4B50 else { throw Failure.truncated("central directory") }
            let method = data.u16(p + 10)
            var compressed = Int(data.u32(p + 20))
            var uncompressed = Int(data.u32(p + 24))
            let nameLen = Int(data.u16(p + 28))
            let extraLen = Int(data.u16(p + 30))
            let commentLen = Int(data.u16(p + 32))
            var offset = Int(data.u32(p + 42))
            guard p + 46 + nameLen + extraLen + commentLen <= data.count else { throw Failure.truncated("central directory") }
            let nameBytes = data.subdata(in: data.startIndex + p + 46 ..< data.startIndex + p + 46 + nameLen)
            let name = String(decoding: nameBytes, as: UTF8.self)

            // Zip64 extra field: only the fields whose classic value is
            // 0xFFFFFFFF are present, in this order.
            var e = p + 46 + nameLen
            let extraEnd = e + extraLen
            while e + 4 <= extraEnd {
                let id = data.u16(e)
                let len = Int(data.u16(e + 2))
                if id == 0x0001 {
                    var q = e + 4
                    if uncompressed == 0xFFFF_FFFF, q + 8 <= extraEnd { uncompressed = Int(data.u64(q)); q += 8 }
                    if compressed == 0xFFFF_FFFF, q + 8 <= extraEnd { compressed = Int(data.u64(q)); q += 8 }
                    if offset == 0xFFFF_FFFF, q + 8 <= extraEnd { offset = Int(data.u64(q)); q += 8 }
                }
                e += 4 + len
            }

            entries[name] = Entry(
                name: name, method: method, compressedSize: compressed,
                uncompressedSize: uncompressed, localHeaderOffset: offset)
            p += 46 + nameLen + extraLen + commentLen
        }
        return entries
    }
}

extension Data {
    fileprivate func u16(_ at: Int) -> UInt16 {
        let i = startIndex + at
        return UInt16(self[i]) | UInt16(self[i + 1]) << 8
    }

    fileprivate func u32(_ at: Int) -> UInt32 {
        UInt32(u16(at)) | UInt32(u16(at + 2)) << 16
    }

    fileprivate func u64(_ at: Int) -> UInt64 {
        UInt64(u32(at)) | UInt64(u32(at + 4)) << 32
    }
}
