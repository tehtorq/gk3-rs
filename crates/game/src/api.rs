//! The game's side of the Sheep API.
//!
//! Functions fall into three groups. **State** functions (flags, variables,
//! counts, inventory, time and place) are performed here, because they decide
//! what the story allows. **Presentation** functions (cameras, animation,
//! dialogue, sound) are recorded in a trace for the engine to perform; those
//! the original lets a script wait on report themselves pending. **Calls**
//! between scripts are handed back to the machine to run. Anything else is
//! counted as unimplemented, so a silent zero never passes for an answer.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use gk3_sheep::vm::CallContext;
use gk3_sheep::{Host, Outcome, Script, ThreadId, Type, Value};

use crate::state::{GameState, character, key};

/// Functions a script can wait on, from G-Engine's registrations.
pub const WAITABLE: &[&str] = &[
    "ActionWaitClearRegion",
    "Call",
    "CallDefaultSheep",
    "CallGlobal",
    "CallGlobalSheep",
    "CallSceneFunction",
    "CallSheep",
    "ClearRegion",
    "ContinueDialogue",
    "ContinueDialogueNoFidgets",
    "DefaultInspect",
    "EndConversation",
    "FollowOnDrivingMap",
    "GlideToCameraAngle",
    "GlideToCameraAngleX",
    "InspectObject",
    "PlayFullScreenMovie",
    "PlayFullScreenMovieX",
    "PlayMovie",
    "PlaySound",
    "PlaySoundTrack",
    "SetConversation",
    "SetIdleGAS",
    "SetListenGAS",
    "SetLocation",
    "SetLocationTime",
    "SetPamphletPage",
    "SetTalkGAS",
    "SetTime",
    "SetTimerMs",
    "SetTimerSeconds",
    "StartAnimation",
    "StartDialogue",
    "StartDialogueNoFidgets",
    "StartIdleFidget",
    "StartListenFidget",
    "StartMom",
    "StartMorphAnimation",
    "StartMoveAnimation",
    "StartTalkFidget",
    "StartVoiceOver",
    "StartYak",
    "StopFidget",
    "StopPropFidget",
    "TurnHead",
    "TurnToModel",
    "Uninspect",
    "WalkNear",
    "WalkNearModel",
    "WalkTo",
    "WalkToAnimation",
    "WalkToSeeModel",
    "Warp",
];

/// Presentation functions: performed by the engine, not by this module.
const PRESENTATION: &[&str] = &[
    "CutToCameraAngle",
    "StopMorphAnimation",
    "PlayMovie",
    "ClearRegion",
    "InspectObject",
    "DefaultInspect",
    "InspectModelUsingAngle",
    "Uninspect",
    "LookitActorQuick",
    "LookitModelQuick",
    "LookitModelX",
    "LookitModelQuickX",
    "LookitNoun",
    "LookitNounQuick",
    "LookitPlayer",
    "LookitSceneModel",
    "LookitSceneModelQuick",
    "LookitMouse",
    "LookitLock",
    "LookitUnlock",
    "EnableEyeJitter",
    "DisableEyeJitter",
    "EyeJitter",
    "ForceCutToCameraAngle",
    "GlideToCameraAngle",
    "GlideToCameraAngleX",
    "SetCameraAngleType",
    "SetDefaultDialogueCamera",
    "ClearDefaultDialogueCamera",
    "SetForcedCameraCuts",
    "ClearForcedCameraCuts",
    "SetCameraFOV",
    "InspectModelUsingAngle",
    "CameraBoundaryBlockModel",
    "CameraBoundaryUnblockModel",
    "EnableCameraBoundaries",
    "DisableCameraBoundaries",
    "StartAnimation",
    "StartMoveAnimation",
    "StopAnimation",
    "LoopAnimation",
    "StartMorphAnimation",
    "StartMom",
    "StartYak",
    "SetMood",
    "ClearMood",
    "TurnHead",
    "TurnToModel",
    "Glance",
    "LookitActor",
    "LookitModel",
    "LookitPoint",
    "LookitCancel",
    "StartDialogue",
    "StartDialogueNoFidgets",
    "ContinueDialogue",
    "ContinueDialogueNoFidgets",
    "StartVoiceOver",
    "AddCaptionVoiceOver",
    "SetConversation",
    "EndConversation",
    "ShowModel",
    "HideModel",
    "ShowSceneModel",
    "HideSceneModel",
    "AddModel",
    "AddActor",
    "AddPosition",
    "EnableModelShadow",
    "DisableModelShadow",
    "SetModelLighting",
    "EnableHitTestModel",
    "DisableHitTestModel",
    "ClownShoes",
    "WalkTo",
    "WalkToAnimation",
    "SetActorPosition",
    "InitEgoPosition",
    "SetWalkAnim",
    "WalkerBoundaryBlockRegion",
    "WalkerBoundaryUnblockRegion",
    "WalkerBoundaryBlockModel",
    "WalkerBoundaryUnblockModel",
    "ActionWaitClearRegion",
    "SetBoundaryMap",
    "SetIdleGAS",
    "SetTalkGAS",
    "SetListenGAS",
    "StartIdleFidget",
    "StartTalkFidget",
    "StartListenFidget",
    "StopFidget",
    "StopPropFidget",
    "PlaySound",
    "PlaySoundTrack",
    "StopSoundTrack",
    "StopAllSoundTracks",
    "PlayFullScreenMovie",
    "PlayFullScreenMovieX",
    "SetTimerSeconds",
    "SetTimerMs",
    "SetGameTimer",
    "SetVerbModal",
    "StartVerbCancel",
    "StopVerbCancel",
    "ShowInventory",
    "HideInventory",
    "InventoryInspect",
    "InventoryUninspect",
    "ShowSidney",
    "ShowBinocs",
    "ShowDrivingInterface",
    "FollowOnDrivingMap",
    "ShowFingerprintInterface",
    "ShowDeathLayer",
    "FinishedScreen",
    "QuitApp",
    "PreloadSheep",
    "SetScene",
    "SetSceneNoPreloadTextures",
    "UploadSceneLightmaps",
];

fn is(name: &str, list: &[&str]) -> bool {
    list.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// A presentation call for the engine to perform. When `thread` is set, that
/// script is waiting, and the engine releases it when the effect is over.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub name: String,
    pub args: Vec<Value>,
    pub thread: Option<ThreadId>,
}

impl Effect {
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn string(&self, i: usize) -> String {
        self.args
            .get(i)
            .map(|v| v.string().to_string())
            .unwrap_or_default()
    }

    pub fn int(&self, i: usize) -> i32 {
        self.args.get(i).map(Value::int).unwrap_or(0)
    }

    pub fn float(&self, i: usize) -> f32 {
        self.args.get(i).map(Value::float).unwrap_or(0.0)
    }
}

/// The game's API over a [`GameState`].
/// Where things are in the scene, for the scripts' questions about it.
#[derive(Debug, Clone, Default)]
pub struct Whereabouts {
    /// Each actor, by model and by noun (upper case): where it stands, and
    /// where its walk ends (where it stands, when it is not walking).
    pub actors: HashMap<String, ([f32; 3], [f32; 3])>,
    /// The scene's named positions (upper case).
    pub positions: HashMap<String, [f32; 3]>,
}

impl Whereabouts {
    /// Whether an actor, or the end of its walk, is within `distance` of a
    /// position, as the original measures: straight-line, in all three
    /// dimensions. Unknown names are never near.
    pub fn near(&self, actor: &str, position: &str, distance: f32, walk_end: bool) -> bool {
        let (Some(&(at, goal)), Some(p)) = (self.actors.get(&key(actor)), self.positions.get(&key(position))) else {
            return false;
        };
        let q = if walk_end { goal } else { at };
        let d2: f32 = (0..3).map(|k| (q[k] - p[k]).powi(2)).sum();
        distance >= 0.0 && d2 < distance * distance
    }
}

pub struct GameApi<'a> {
    pub state: &'a mut GameState,
    pub scripts: &'a HashMap<String, Rc<Script>>,
    /// Presentation calls in order, as `Name(args)`.
    pub trace: Vec<String>,
    /// Presentation calls, for an engine to perform.
    pub effects: Vec<Effect>,
    /// Threads waiting on something presentation started, in order.
    pub pending: Vec<ThreadId>,
    pub unimplemented: BTreeMap<String, usize>,
    pub calls: u64,
    /// Actors known to the current scene; empty means "assume they exist".
    pub actors: Vec<String>,
    /// Where the scene's actors and positions are; without it, no one is
    /// near anywhere.
    pub whereabouts: Option<&'a Whereabouts>,
}

impl<'a> GameApi<'a> {
    pub fn new(state: &'a mut GameState, scripts: &'a HashMap<String, Rc<Script>>) -> Self {
        Self {
            state,
            scripts,
            trace: Vec::new(),
            effects: Vec::new(),
            pending: Vec::new(),
            unimplemented: BTreeMap::new(),
            calls: 0,
            actors: Vec::new(),
            whereabouts: None,
        }
    }

    fn query(&mut self, name: &str, a: &[Value]) -> Option<Value> {
        let s = |i: usize| a.get(i).map(|v| v.string()).unwrap_or_default();
        let i = |i: usize| a.get(i).map(Value::int).unwrap_or(0);
        let st = &mut *self.state;
        let ego = st.ego.clone();
        Some(match key(name).as_str() {
            // Flags and variables.
            "GETFLAG" => Value::from(st.flags.contains(&key(&s(0)))),
            "SETFLAG" => {
                st.flags.insert(key(&s(0)));
                Value::Int(0)
            }
            "CLEARFLAG" => {
                st.flags.remove(&key(&s(0)));
                Value::Int(0)
            }
            "GETGAMEVARIABLEINT" => Value::Int(*st.variables.get(&key(&s(0))).unwrap_or(&0)),
            "SETGAMEVARIABLEINT" => {
                st.variables.insert(key(&s(0)), i(1));
                Value::Int(0)
            }
            "INCGAMEVARIABLEINT" => {
                *st.variables.entry(key(&s(0))).or_default() += 1;
                Value::Int(0)
            }
            // Counts: what has been done to what.
            "GETNOUNVERBCOUNT" | "GETNOUNVERBCOUNTINT" => {
                Value::Int(st.noun_verb(&ego, &s(0), &s(1)))
            }
            "SETNOUNVERBCOUNT" => {
                st.set_noun_verb(&ego, &s(0), &s(1), i(2));
                Value::Int(0)
            }
            "INCNOUNVERBCOUNT" => {
                let n = st.noun_verb(&ego, &s(0), &s(1));
                st.set_noun_verb(&ego, &s(0), &s(1), n + 1);
                Value::Int(0)
            }
            "SETNOUNVERBCOUNTBOTH" => {
                for who in ["GABRIEL", "GRACE"] {
                    st.set_noun_verb(who, &s(0), &s(1), i(2));
                }
                Value::Int(0)
            }
            "INCNOUNVERBCOUNTBOTH" => {
                for who in ["GABRIEL", "GRACE"] {
                    let n = st.noun_verb(who, &s(0), &s(1));
                    st.set_noun_verb(who, &s(0), &s(1), n + 1);
                }
                Value::Int(0)
            }
            "GETTOPICCOUNT" | "GETTOPICCOUNTINT" => {
                Value::Int(*st.topics.get(&(key(&s(0)), key(&s(1)))).unwrap_or(&0))
            }
            "SETTOPICCOUNT" => {
                st.topics.insert((key(&s(0)), key(&s(1))), i(2));
                Value::Int(0)
            }
            "GETCHATCOUNT" | "GETCHATCOUNTINT" => {
                Value::Int(*st.chats.get(&key(&s(0))).unwrap_or(&0))
            }
            "SETCHATCOUNT" => {
                st.chats.insert(key(&s(0)), i(1));
                Value::Int(0)
            }
            "CHANGESCORE" => {
                st.score_events.insert(key(&s(0)));
                Value::Int(0)
            }
            // Time and place.
            "ISCURRENTTIME" => Value::from(st.timeblock == key(&s(0))),
            "WASLASTTIME" => Value::from(st.last_timeblock == key(&s(0))),
            "ISCURRENTLOCATION" => Value::from(st.location == key(&s(0))),
            "WASLASTLOCATION" => Value::from(st.last_location == key(&s(0))),
            "GETEGOCURRENTLOCATIONCOUNT" => {
                let location = st.location.clone();
                Value::Int(st.location_count(&ego, &location))
            }
            "GETEGOLOCATIONCOUNT" => Value::Int(st.location_count(&ego, &s(0))),
            "WASEGOEVERINLOCATION" => Value::from(st.ever_visited(&ego, &s(0))),
            "ISACTORATLOCATION" => Value::from(
                st.actor_locations
                    .get(&character(&s(0)))
                    .is_some_and(|l| *l == key(&s(1))),
            ),
            "SETACTORLOCATION" => {
                st.actor_locations.insert(character(&s(0)), key(&s(1)));
                Value::Int(0)
            }
            "ISCURRENTEGO" => Value::from(st.ego == character(&s(0))),
            "SETEGO" => {
                st.ego = character(&s(0));
                Value::Int(0)
            }
            // Inventory: per character.
            "DOESEGOHAVEINVITEM" => Value::from(st.has_item(&ego, &s(0))),
            "DOESGABEHAVEINVITEM" => Value::from(st.has_item("GABRIEL", &s(0))),
            "DOESGRACEHAVEINVITEM" => Value::from(st.has_item("GRACE", &s(0))),
            "EGOTAKEINVITEM" => {
                st.give_item(&ego, &s(0));
                st.active_item.insert(ego, key(&s(0)));
                Value::Int(0)
            }
            "SETEGOACTIVEINVITEM" => {
                st.active_item.insert(ego, key(&s(0)));
                Value::Int(0)
            }
            "ISACTIVEINVITEM" => Value::from(st.active_item.get(&ego) == Some(&key(&s(0)))),
            "SETINVITEMSTATUS" => {
                let item = s(0);
                st.take_item("GABRIEL", &item);
                st.take_item("GRACE", &item);
                match key(&s(1)).as_str() {
                    "GABEHAS" => st.give_item("GABRIEL", &item),
                    "GRACEHAS" => st.give_item("GRACE", &item),
                    "BOTHHAVE" => {
                        st.give_item("GABRIEL", &item);
                        st.give_item("GRACE", &item);
                    }
                    _ => {}
                }
                Value::Int(0)
            }
            "COMBINEINVITEMS" => {
                st.take_item(&ego, &s(0));
                st.take_item(&ego, &s(1));
                st.give_item(&ego, &s(2));
                Value::Int(0)
            }
            "GETRANDOMINT" => Value::Int(st.random(i(0), i(1))),
            // Questions about the scene that the engine will answer properly.
            "DOESACTOREXIST" => Value::from(
                self.actors.is_empty() || self.actors.iter().any(|a| a.eq_ignore_ascii_case(&s(0))),
            ),
            "DOESMODELEXIST" => Value::Int(1),
            "ISCAMERAGLIDEENABLED" => Value::Int(1),
            "ISACTORNEAR" | "ISWALKINGACTORNEAR" => {
                let distance = a.get(2).map_or(0.0, Value::float);
                let walk_end = key(name) == "ISWALKINGACTORNEAR";
                Value::from(
                    self.whereabouts
                        .is_some_and(|w| w.near(&s(0), &s(1), distance, walk_end)),
                )
            }
            "ISTOPLAYERINVENTORY" => Value::from(st.inventory_shown),
            "DOESSIDNEYFILEEXIST" => Value::Int(0),
            _ => return None,
        })
    }
}

impl Host for GameApi<'_> {
    fn call(&mut self, name: &str, args: &[Value], _: Type, context: &CallContext) -> Outcome {
        self.calls += 1;
        let s = |i: usize| {
            args.get(i)
                .map(|v| v.string().to_string())
                .unwrap_or_default()
        };

        // Calls between scripts run as Sheep threads.
        match key(name).as_str() {
            "CALLSHEEP" | "CALLGLOBALSHEEP" | "CALLDEFAULTSHEEP" => {
                return Outcome::Spawn {
                    script: key(&s(0)),
                    function: with_dollar(&s(1)),
                };
            }
            "CALL" | "CALLGLOBAL" => {
                return Outcome::Spawn {
                    script: String::new(),
                    function: with_dollar(&s(0)),
                };
            }
            _ => {}
        }
        if key(name) == "SETLOCATION" {
            // The visit is counted when the room is entered.
            self.state.move_to(&s(0));
        }
        if let Some(value) = self.query(name, args) {
            return Outcome::Done(value);
        }

        // Scene functions are the engine's own code, not Sheep.
        let presentation = is(name, PRESENTATION)
            || matches!(key(name).as_str(), "SETLOCATION" | "CALLSCENEFUNCTION");
        if presentation {
            let shown: Vec<String> = args.iter().map(ToString::to_string).collect();
            self.trace.push(format!("{name}({})", shown.join(", ")));
        } else {
            *self.unimplemented.entry(name.to_string()).or_default() += 1;
        }
        // Only what the engine performs can release a waiting script: an
        // unknown call is over at once, or its script would wait for ever.
        let waits = presentation
            && is(name, WAITABLE)
            && context.in_wait
            && context.thread != usize::MAX;
        if presentation {
            self.effects.push(Effect {
                name: name.to_string(),
                args: args.to_vec(),
                thread: waits.then_some(context.thread),
            });
        }
        if waits {
            self.pending.push(context.thread);
            return Outcome::Pending(Value::Int(0));
        }
        Outcome::Done(Value::Int(0))
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        self.scripts.get(&key(name)).cloned()
    }
}

fn with_dollar(name: &str) -> String {
    if name.ends_with('$') {
        name.to_string()
    } else {
        format!("{name}$")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gk3_sheep::expr::Expr;

    fn eval(api: &mut GameApi, text: &str) -> i32 {
        Expr::parse(text).unwrap().eval(api, &|_| None).int()
    }

    #[test]
    fn state_functions_round_trip() {
        let scripts = HashMap::new();
        let mut state = GameState::new("110a", "R25");
        let mut api = GameApi::new(&mut state, &scripts);
        assert_eq!(
            eval(&mut api, r#"IsCurrentTime("110A") && !GetFlag("Door")"#),
            1
        );
        eval(&mut api, r#"SetFlag("door")"#);
        assert_eq!(eval(&mut api, r#"GetFlag("DOOR")"#), 1);
        eval(&mut api, r#"IncNounVerbCount("Window", "Open")"#);
        eval(&mut api, r#"IncNounVerbCount("Window", "Open")"#);
        assert_eq!(eval(&mut api, r#"GetNounVerbCount("window", "open")"#), 2);
        eval(&mut api, r#"SetInvItemStatus("Wallet", "BothHave")"#);
        assert_eq!(
            eval(
                &mut api,
                r#"DoesGraceHaveInvItem("wallet") && DoesEgoHaveInvItem("wallet")"#
            ),
            1
        );
        eval(&mut api, r#"SetInvItemStatus("Wallet", "Used")"#);
        assert_eq!(eval(&mut api, r#"DoesEgoHaveInvItem("wallet")"#), 0);
        assert!(api.unimplemented.is_empty());
    }

    #[test]
    fn presentation_is_traced_and_unknowns_counted() {
        let scripts = HashMap::new();
        let mut state = GameState::default();
        let mut api = GameApi::new(&mut state, &scripts);
        eval(&mut api, r#"CutToCameraAngle("START")"#);
        eval(&mut api, r#"FrobnicateWidget(1)"#);
        assert_eq!(api.trace, [r#"CutToCameraAngle("START")"#]);
        assert_eq!(api.unimplemented.get("FrobnicateWidget"), Some(&1));
    }

    /// A script waiting on a call the engine cannot perform goes on at
    /// once: nothing would ever release it.
    #[test]
    fn only_performed_calls_are_waited_on() {
        let scripts = HashMap::new();
        let mut state = GameState::default();
        let mut api = GameApi::new(&mut state, &scripts);
        let waiting = CallContext {
            thread: 1,
            script: "TEST",
            in_wait: true,
        };
        // Waitable, but not the engine's to perform here.
        let unknown = api.call("Warp", &[], Type::Void, &waiting);
        assert!(matches!(unknown, Outcome::Done(_)));
        // Performed by the engine, which releases the script when it is over.
        let walk = api.call("StartAnimation", &[Value::from("X")], Type::Void, &waiting);
        assert!(matches!(walk, Outcome::Pending(_)));
        assert_eq!(api.effects.last().map(|e| e.thread), Some(Some(1)));
    }
}
