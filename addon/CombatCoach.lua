-- CombatCoach.lua
-- Thin identity-only addon for the CombatLedger Live Coach companion app.
--
-- PURPOSE: Write the player's GUID, name, realm, class, and spec to SavedVariables
-- so the companion app can target the right player in the combat log without
-- any manual configuration.
--
-- This addon does NO combat log processing — all parsing and analysis happens
-- in the companion app (Rust backend). Keeping this addon minimal ensures it
-- is compatible across patches and cannot cause taint issues.
--
-- SavedVariables file location:
--   WTF/Account/<ACCOUNT>/SavedVariables/CombatCoach.lua
-- The companion app watches this file for changes via a file system watcher.
--
-- LIMITATION: WoW only flushes SavedVariables to disk on logout or /reload.
-- The companion app handles this by also inferring the player GUID from the
-- combat log itself (the player's GUID appears in their cast events) as a fallback.

local ADDON_NAME = "CombatCoach"
local VERSION    = "0.1.0"

-- ============================================================
-- WriteIdentity — called on login, spec change, and talent update
-- ============================================================
local function WriteIdentity()
    if not CombatCoachDB then
        CombatCoachDB = {}
    end

    local guid            = UnitGUID("player") or ""
    local name, realm     = UnitFullName("player")
    local _, className    = UnitClass("player")
    local specIndex       = GetSpecialization()
    local specName        = ""
    local specRole        = ""

    if specIndex then
        local _, rawSpecName, _, _, _, role = GetSpecializationInfo(specIndex)
        specName = rawSpecName or ""
        specRole = role or ""  -- "DAMAGER", "HEALER", "TANK"
    end

    -- UnitFullName may return nil realm on home realm characters
    if not realm or realm == "" then
        realm = GetRealmName() or ""
    end

    name = name or UnitName("player") or ""

    CombatCoachDB["playerGUID"]   = guid
    CombatCoachDB["playerName"]   = name
    CombatCoachDB["realmName"]    = realm
    CombatCoachDB["className"]    = className or ""
    CombatCoachDB["specName"]     = specName
    CombatCoachDB["specRole"]     = specRole   -- useful for role-aware coaching rules
    CombatCoachDB["addonVersion"] = VERSION
    CombatCoachDB["updatedAt"]    = GetServerTime()
end

-- ============================================================
-- Event registration
-- ============================================================
local frame = CreateFrame("Frame", ADDON_NAME .. "Frame", UIParent)

frame:RegisterEvent("PLAYER_LOGIN")
frame:RegisterEvent("PLAYER_SPECIALIZATION_CHANGED")
frame:RegisterEvent("PLAYER_TALENT_UPDATE")  -- Hero talent changes in TWW

frame:SetScript("OnEvent", function(self, event, ...)
    if event == "PLAYER_LOGIN" then
        -- Small delay: UnitFullName can return nil realm during the login sequence
        -- before the client has fully initialised the character data.
        C_Timer.After(2.0, WriteIdentity)
    else
        WriteIdentity()
    end
end)

-- ============================================================
-- Slash commands — manual refresh and debug output
-- ============================================================
SLASH_COMBATCOACH1 = "/combatcoach"
SLASH_COMBATCOACH2 = "/cc"

SlashCmdList["COMBATCOACH"] = function(msg)
    local cmd = strtrim(msg):lower()

    if cmd == "status" or cmd == "" then
        WriteIdentity()
        local db = CombatCoachDB or {}
        print(string.format(
            "|cff7c5cffCombatCoach|r v%s — Identity written:",
            VERSION
        ))
        print(string.format(
            "  Name: %s @ %s",
            db["playerName"] or "?",
            db["realmName"]  or "?"
        ))
        print(string.format(
            "  GUID: %s",
            db["playerGUID"] or "?"
        ))
        print(string.format(
            "  Class/Spec: %s / %s (%s)",
            db["className"] or "?",
            db["specName"]  or "?",
            db["specRole"]  or "?"
        ))
        print(string.format(
            "  SavedVariables: WTF/Account/.../SavedVariables/CombatCoach.lua"
        ))
        print("|cff7c5cffTip:|r Use /reload to flush SavedVariables to disk immediately.")

    elseif cmd == "reset" then
        CombatCoachDB = {}
        print("|cff7c5cffCombatCoach|r — Database reset.")

    else
        print("|cff7c5cffCombatCoach|r commands:")
        print("  /cc status  — refresh and display identity")
        print("  /cc reset   — clear saved data")
    end
end
