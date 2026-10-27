import Foundation
import FoundationModels

/// On-device helper for VRCLibMan. The app stays on macOS 14; this process is
/// built on a Mac whose SDK includes FoundationModels. Private Cloud Compute is
/// not used: `SystemLanguageModel.default` is the on-device model.
///
/// `--availability` prints one JSON object and exits.
/// Otherwise the process stays up and reads one JSON object per line so the
/// model can stay loaded.

@Generable
struct BoothExtraction {
    @Guide(description: "One of avatar, outfit, hair, accessory, texture, gimmick, world, other")
    var category: String
    @Guide(description: "One of outfit, shoes_socks, hair, accessory, head_ears_tail, makeup_skin, eye_texture, expression_animation, pose, gimmick, particle_effect, shader, tool_editor, other, world, furniture_props, world_gimmick, texture_material, skybox, lighting_postprocess, world_shader, building_terrain, audio_bgm, tool_other")
    var genre: String
    @Guide(description: "Avatar names written in the text. Keep a version suffix such as ver1.2 with the name. Do not include shader or tool names.")
    var compatibleAvatars: [String]
    var isGeneric: Bool
    var confidence: Double
}

@main
struct AfmMain {
    static func main() async {
        if CommandLine.arguments.contains("--availability") {
            await printAvailability()
        } else {
            await serve()
        }
    }
}

private func printAvailability() async {
    let model = SystemLanguageModel.default
    var availability = "unavailable"
    var reason = ""
    switch model.availability {
    case .available:
        availability = "available"
    case .unavailable(let why):
        availability = "unavailable"
        reason = String(describing: why)
    }
    let payload: [String: Any] = [
        "availability": availability,
        "variant": "default",
        "contextSize": model.contextSize,
        "reason": reason,
    ]
    emit(payload)
}

private func serve() async {
    let model = SystemLanguageModel.default
    while let line = readLine(strippingNewline: true) {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            continue
        }
        guard let data = trimmed.data(using: .utf8),
              let request = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            emit(["ok": false, "error": "badRequest", "message": "依頼のJSONを読めません"])
            continue
        }
        let title = request["title"] as? String ?? ""
        let description = request["description"] as? String ?? ""
        let instructions = request["instructions"] as? String ?? ""
        let response = await generate(model: model, instructions: instructions, title: title, description: description)
        emit(response)
    }
}

private func generate(
    model: SystemLanguageModel,
    instructions: String,
    title: String,
    description: String
) async -> [String: Any] {
    guard case .available = model.availability else {
        return ["ok": false, "error": "unavailable", "message": "Apple のオンデバイスモデルは今は使えません"]
    }
    let body = await clip(model: model, instructions: instructions, title: title, description: description)
    let user = "タイトル: \(title)\n説明文:\n\(body)"
    let session = LanguageModelSession(model: model, instructions: instructions)
    let options = GenerationOptions(samplingMode: .greedy, temperature: 0.1)
    do {
        let response = try await session.respond(
            to: user,
            generating: BoothExtraction.self,
            options: options
        )
        let extracted = response.content
        let json: [String: Any] = [
            "category": extracted.category,
            "genre": extracted.genre,
            "compatible_avatars": extracted.compatibleAvatars,
            "is_generic": extracted.isGeneric,
            "confidence": extracted.confidence,
        ]
        return ["ok": true, "json": json]
    } catch {
        return ["ok": false, "error": errorCode(error), "message": "Apple のオンデバイスモデルが応答できません"]
    }
}

private func clip(
    model: SystemLanguageModel,
    instructions: String,
    title: String,
    description: String
) async -> String {
    let reserve = 768
    let budget = max(512, model.contextSize - reserve)
    var body = description
    while true {
        let whole = instructions + "\nタイトル: " + title + "\n説明文:\n" + body
        let count = (try? await model.tokenCount(for: whole)) ?? whole.count
        if count <= budget || body.isEmpty {
            return body
        }
        let next = body.count * 3 / 4
        if next >= body.count {
            return String(body.prefix(max(0, budget)))
        }
        body = String(body.prefix(next))
    }
}

private func errorCode(_ error: Error) -> String {
    if let typed = error as? LanguageModelError {
        switch typed {
        case .guardrailViolation(_):
            return "guardrailViolation"
        case .contextSizeExceeded(_):
            return "contextWindow"
        default:
            break
        }
    }
    return "failed"
}

private func emit(_ payload: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
          let line = String(data: data, encoding: .utf8)
    else {
        print(#"{"ok":false,"error":"failed","message":"応答を書けません"}"#)
        fflush(stdout)
        return
    }
    print(line)
    fflush(stdout)
}
