-- CombatCoach.lua
-- Companion addon for the CombatLedger Live Coach app.
--
-- PURPOSE:
--   1. Write the player's GUID, name, realm, class, and spec to SavedVariables
--      so the companion app can target the right player in the combat log without
--      any manual configuration.
--
--   2. Flush the combat log to disk once per second during combat by cycling
--      LoggingCombat(false) → LoggingCombat(true).  WoW buffers log writes in
--      its C-runtime stdio buffer (~64-128 KB); on low-intensity content (target
--      dummies, open-world) the buffer never fills during a pull, so all events
--      reach disk only when combat ends.  Cycling LoggingCombat closes the
--      current file (flushing the OS buffer) and opens a new one, which the
--      companion app's filesystem watcher detects as a Create event within ~50 ms.
--
-- This addon does NO combat log processing — all parsing and analysis happens
-- in the companion app (Rust backend).  Keeping this addon minimal ensures it
-- is compatible across patches and cannot cause taint issues.
--
-- SavedVariables file location:
--   WTF/Account/<ACCOUNT>/SavedVariables/CombatCoach.lua
--
-- VERSION HISTORY:
--   1.0.0 — Identity-only (GUID/name/class/spec on login/spec-change)
--   1.1.0 — Added 1-second combat-log flush for real-time coaching

local ADDON_NAME = "CombatCoach"
local VERSION    = "1.1.0"

-- ============================================================
-- Combat-log flush state
-- ============================================================

-- Set to true while the player is in combat.  Updated by PLAYER_REGEN_DISABLED
-- (enter combat) and PLAYER_REGEN_ENABLED (leave combat).
local inCombat = false

-- ============================================================
-- FlushCombatLog — called by the 1-second ticker
-- ============================================================
-- Cycles LoggingCombat off/on to force WoW to close the current log file
-- (flushing the write buffer to disk) and open a fresh one.  The companion
-- app's tailer detects the new file via ReadDirectoryChangesW within ~50 ms.
--
-- This is a no-op when the player is not in combat or when combat logging is
-- disabled, so it is safe to keep the ticker running at all times.
local function FlushCombatLog(force)
    if not (force or inCombat) then return end
    if not LoggingCombat() then return end

    -- Disable logging → WoW closes the file and flushes its write buffer.
    -- Enable  logging → WoW opens a new timestamped WoWCombatLog-*.txt file.
    LoggingCombat(false)
    LoggingCombat(true)
end

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
frame:RegisterEvent("PLAYER_REGEN_DISABLED") -- Entered combat
frame:RegisterEvent("PLAYER_REGEN_ENABLED")  -- Left combat

frame:SetScript("OnEvent", function(self, event, ...)
    if event == "PLAYER_LOGIN" then
        -- Small delay: UnitFullName can return nil realm during the login sequence
        -- before the client has fully initialised the character data.
        C_Timer.After(2.0, WriteIdentity)

        -- Sync combat state in case the player logged in while already in combat
        -- (e.g. a disconnect/reconnect during a fight).
        inCombat = UnitAffectingCombat("player") == true

        -- Start the 1-second flush ticker.  The ticker runs for the entire session
        -- but FlushCombatLog() returns immediately when not in combat, so the
        -- performance overhead is negligible.
        C_Timer.NewTicker(1.0, FlushCombatLog)

    elseif event == "PLAYER_SPECIALIZATION_CHANGED" or event == "PLAYER_TALENT_UPDATE" then
        WriteIdentity()

    elseif event == "PLAYER_REGEN_DISABLED" then
        -- Player entered combat.  The ticker will start flushing on its next tick.
        inCombat = true

    elseif event == "PLAYER_REGEN_ENABLED" then
        -- Player left combat.  Do one final flush now to capture the last events
        -- (the ticker may not have fired for up to 1 second before combat ended).
        inCombat = false
        FlushCombatLog(true)  -- force=true: flush even though inCombat is now false
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
            "  Log flush: %s | In combat: %s",
            LoggingCombat() and "|cff00ff00active|r" or "|cffff0000disabled|r",
            inCombat        and "|cffff8800yes|r"    or "|cff888888no|r"
        ))
        print("|cff7c5cffTip:|r Use /cc flush to manually flush the combat log.")

    elseif cmd == "flush" then
        if LoggingCombat() then
            FlushCombatLog(true)
            print("|cff7c5cffCombatCoach|r — Combat log flushed.")
        else
            print("|cff7c5cffCombatCoach|r — Combat logging is not enabled. Use /combatlog to enable it first.")
        end

    elseif cmd == "reset" then
        CombatCoachDB = {}
        print("|cff7c5cffCombatCoach|r — Database reset.")

    else
        print("|cff7c5cffCombatCoach|r commands:")
        print("  /cc status  — refresh and display identity + flush status")
        print("  /cc flush   — manually flush the combat log to disk")
        print("  /cc reset   — clear saved data")
    end
end
