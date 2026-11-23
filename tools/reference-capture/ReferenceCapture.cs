// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
//
// BepInEx plugin for CharaStudio. On a hotkey it works through a list of
// cases, saves pictures of each and a JSON file describing the camera,
// lights, global shader values, pose and drawn materials. Renkatsu uses
// these as the reference when checking its own renders.
//
// Ctrl+Shift+F9  reads UserData/renkatsu/cards.txt, writes to out/
// Ctrl+Shift+F10 reads UserData/renkatsu/batch.txt, writes to out/batch/
//
// A list line is a card path (relative paths start at UserData/chara/female,
// "-" is no character), then fields separated by "|":
//   3 or outfit=3        wear outfit 3; files are named <name>.outfit3
//   name=legs            added to the file names: <name>.legs
//   view=body,face       pictures to take: body, face, back, side
//   zoom=n:bone:half[:yaw[:pitch]]  a picture named n that looks at a bone
//                        ("a+b" is the middle of two) and shows `half`
//                        units above and below it
//   cam=n:x,y,z:x,y,z[:fov]  a picture named n from a place towards a point
//   res=1800x2800        picture size (900x1400 without it)
//   msaa=1               samples per pixel of the picture (8 without it)
//   pose=0/3/2           an animation of Studio's list, held still;
//                        pose=maker is the character maker's first pose
//   time=0.5             where in the animation, in passes
//   frames=8, step=3     then let it play and take 8 more captures, one
//                        every 3 frames of 1/30 s: <name>.f00 to .f08
//   clothes=top:1,bot:3  clothing states (0 on, 1 and 2 half, 3 off)
//   map=myroom           load a map, by scene file name or number
//   sun=day|evening|night  the map's time of day
//   settle=90            frames to wait before the picture
//   plain                skip the pictures with Studio's effects
//   ik=on                switch on IK with its targets on the bones, as
//                        Studio's copy button puts them, and write what
//                        the solver read, worked out and wrote to
//                        <name>.ik.json
//   ik=LeftHand:x,y,z[:x,y,z]  the same, with that target (a name of
//                        OICharInfo.IKTargetEN or its number) moved by
//                        x,y,z in the world and turned by Euler angles
//   fk=on                switch on FK with the animation copied into it
//   fk=cf_j_neck:x,y,z   the same, with that bone's Euler angles in
//                        Studio's FK fields raised by x,y,z
// Without a list, the default card.
//
// Output: <name>.<view>.studio.png  with Studio's effects
//         <name>.<view>.plain.png   image effects off
//         <name>.json
//         <name>.ik.json            with ik=
//         index.txt (batch only): one line per case
//         The views are "body" and "face" unless the line says otherwise.

using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using BepInEx;
using BepInEx.Configuration;
using Studio;
using UnityEngine;

namespace Renkatsu
{
    // Carries Renkatsu's own version (Cargo.toml), not a separate number.
    [BepInPlugin("renkatsu.referencecapture", "Renkatsu reference capture", "0.1.8")]
    [BepInProcess("CharaStudio")]
    public class ReferenceCapture : BaseUnityPlugin
    {
        private const int DefaultWidth = 900;
        private const int DefaultHeight = 1400;
        private const int SettleFrames = 90;
        // Frames per second while a sequence plays, so every run steps alike.
        private const int SequenceRate = 30;
        private const string MakerBundle = "custom/custompose.unity3d";
        private const string MakerController = "custom_pose";
        private const string MakerState = "Stand_D_00";

        private ConfigEntry<KeyboardShortcut> hotkey;
        private ConfigEntry<KeyboardShortcut> batchHotkey;
        private ConfigEntry<KeyboardShortcut> mathHotkey;
        private bool running;

        private void Awake()
        {
            hotkey = Config.Bind("General", "Capture hotkey", new KeyboardShortcut(KeyCode.F9, KeyCode.LeftControl, KeyCode.LeftShift),
                "Loads every card from UserData/renkatsu/cards.txt and saves reference pictures.");
            batchHotkey = Config.Bind("General", "Batch hotkey", new KeyboardShortcut(KeyCode.F10, KeyCode.LeftControl, KeyCode.LeftShift),
                "Runs every case of UserData/renkatsu/batch.txt and saves to out/batch.");
            mathHotkey = Config.Bind("General", "Unity maths hotkey", new KeyboardShortcut(KeyCode.F11, KeyCode.LeftControl, KeyCode.LeftShift),
                "Saves Unity's answers for the rotation and vector functions the IK solver uses to out/unitymath.json.");
        }

        private void Update()
        {
            if (running || !Singleton<Studio.Studio>.IsInstance())
            {
                return;
            }
            if (hotkey.Value.IsDown())
            {
                StartCoroutine(Run("cards.txt", ""));
            }
            else if (batchHotkey.Value.IsDown())
            {
                StartCoroutine(Run("batch.txt", "batch"));
            }
            else if (mathHotkey.Value.IsDown())
            {
                UnityMath();
            }
        }

        // What the batch changes in the scene besides its own objects.
        private class Saved
        {
            public bool fog;
            public FogMode fogMode;
            public Color fogColor;
            public float fogStart;
            public float fogEnd;
            public Texture lut;
            public Texture lutBlend;
            public float blend;
            public int sun;
            public bool vanish;
        }

        private IEnumerator Run(string list, string sub)
        {
            running = true;
            string root = Path.Combine(Paths.GameRootPath, "UserData/renkatsu");
            string outDir = Path.Combine(root, "out");
            if (sub.Length > 0)
            {
                outDir = Path.Combine(outDir, sub);
            }
            Directory.CreateDirectory(outDir);
            List<Entry> entries = ReadList(Path.Combine(root, list));
            Logger.LogMessage("Reference capture: " + entries.Count + " cases from " + list);
            Studio.Studio studio = Singleton<Studio.Studio>.Instance;
            StringBuilder index = new StringBuilder();
            float startedAll = Time.realtimeSinceStartup;

            Saved saved = null;
            string mapNow = null;
            OCICharFemale chara = null;
            string loaded = null;
            bool reload = false;
            bool posed = false;
            bool undressed = false;
            int firstOutfit = 0;
            int[] firstAnime = new int[3];
            float firstSpeed = 1f;
            int done = 0;

            for (int n = 0; n < entries.Count; n++)
            {
                Entry e = entries[n];
                float began = Time.realtimeSinceStartup;
                string name = CaseName(e);

                if (e.Map != mapNow)
                {
                    if (saved == null)
                    {
                        saved = SaveScene(studio);
                        // Walls hidden for Studio's own camera would be
                        // missing from pictures taken from other places.
                        studio.cameraCtrl.isConfigVanish = false;
                    }
                    int no = -1;
                    bool found = false;
                    try
                    {
                        no = e.Map == null ? -1 : FindMap(e.Map);
                        found = e.Map == null || no >= 0;
                        if (found)
                        {
                            studio.AddMap(no, false, false, true);
                        }
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not load map " + e.Map + ": " + ex);
                        found = false;
                    }
                    if (!found)
                    {
                        Logger.LogWarning("Reference capture: no map " + e.Map);
                        index.Append(name).Append("\tno map ").Append(e.Map).Append("\n");
                        continue;
                    }
                    int waited = 0;
                    do
                    {
                        yield return null;
                        waited++;
                    }
                    while (waited < 1800 && (Singleton<Map>.Instance.isLoading || (no >= 0 && Singleton<Map>.Instance.mapRoot == null)));
                    for (int i = 0; i < 30; i++)
                    {
                        yield return null;
                    }
                    mapNow = e.Map;
                }
                if (e.Map != null)
                {
                    try
                    {
                        Singleton<Map>.Instance.sunType = (SunLightInfo.Info.Type)e.Sun;
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not set the time of day: " + ex);
                    }
                }

                if (e.Path != loaded || reload)
                {
                    if (chara != null)
                    {
                        studio.treeNodeCtrl.DeleteNode(chara.treeNodeObject);
                        chara = null;
                        for (int i = 0; i < 5; i++)
                        {
                            yield return null;
                        }
                    }
                    loaded = null;
                    reload = false;
                    posed = false;
                    undressed = false;
                    if (e.Path != null)
                    {
                        if (!File.Exists(e.Path))
                        {
                            Logger.LogWarning("Reference capture: missing " + e.Path);
                            index.Append(name).Append("\tmissing card\n");
                            continue;
                        }
                        try
                        {
                            chara = AddObjectFemale.Add(e.Path);
                        }
                        catch (Exception ex)
                        {
                            Logger.LogError("Reference capture: could not load " + e.Path + ": " + ex);
                        }
                        if (chara == null)
                        {
                            index.Append(name).Append("\tcould not load\n");
                            continue;
                        }
                        loaded = e.Path;
                        firstOutfit = chara.charInfo.fileStatus.coordinateType;
                        firstAnime[0] = chara.oiCharInfo.animeInfo.group;
                        firstAnime[1] = chara.oiCharInfo.animeInfo.category;
                        firstAnime[2] = chara.oiCharInfo.animeInfo.no;
                        firstSpeed = chara.animeSpeed;
                    }
                }

                if (chara != null)
                {
                    try
                    {
                        if (e.Outfit >= 0)
                        {
                            chara.SetCoordinateInfo((ChaFileDefine.CoordinateType)e.Outfit, true);
                        }
                        else if (chara.charInfo.fileStatus.coordinateType != firstOutfit)
                        {
                            chara.SetCoordinateInfo((ChaFileDefine.CoordinateType)firstOutfit, true);
                        }
                        if (undressed)
                        {
                            chara.charInfo.SetClothesStateAll(0);
                            undressed = false;
                        }
                        foreach (KeyValuePair<int, int> state in e.Clothes)
                        {
                            chara.SetClothesState(state.Key, (byte)state.Value);
                            undressed = true;
                        }
                        if (e.HasPose)
                        {
                            if (e.Pose[2] < 0)
                            {
                                // Studio does not list the maker's pose.
                                chara.charInfo.LoadAnimation(MakerBundle, MakerController, "");
                                chara.charInfo.animBody.Play(MakerState, 0, e.Time);
                                chara.charInfo.setAnimatorParamFloat("height", chara.charInfo.fileBody.shapeValueBody[0]);
                                // Studio no longer knows which controller is on.
                                reload = true;
                            }
                            else
                            {
                                chara.LoadAnime(e.Pose[0], e.Pose[1], e.Pose[2], e.Time);
                                chara.charInfo.animBody.Play(chara.charAnimeCtrl.nameHadh, 0, e.Time);
                            }
                            chara.animeSpeed = 0f;
                            posed = true;
                        }
                        else if (posed)
                        {
                            chara.LoadAnime(firstAnime[0], firstAnime[1], firstAnime[2], 0f);
                            chara.animeSpeed = firstSpeed;
                            posed = false;
                        }
                        // A blink in the middle of a picture would make runs differ.
                        chara.charInfo.ChangeEyesBlinkFlag(false);
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not set up " + name + ": " + ex);
                    }
                }
                for (int i = 0; i < e.Settle; i++)
                {
                    yield return null;
                }
                IkWatch watch = null;
                if (chara != null && (e.Ik || e.Fk))
                {
                    bool set = false;
                    try
                    {
                        Kinematics(chara, e);
                        set = true;
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not set up IK or FK for " + name + ": " + ex);
                    }
                    for (int i = 0; set && i < KinematicFrames; i++)
                    {
                        yield return null;
                    }
                    // The solver is watched through the frame of the picture.
                    if (set && e.Ik)
                    {
                        watch = new IkWatch(chara);
                    }
                    yield return null;
                }
                yield return new WaitForEndOfFrame();

                bool ok = false;
                try
                {
                    Capture(chara, e, Path.Combine(outDir, e.Frames > 0 ? name + ".f00" : name), 0);
                    ok = true;
                    done++;
                }
                catch (Exception ex)
                {
                    Logger.LogError("Reference capture: failed on " + name + ": " + ex);
                }
                if (watch != null)
                {
                    watch.Stop();
                    try
                    {
                        File.WriteAllText(Path.Combine(outDir, name + ".ik.json"), watch.Json(), new UTF8Encoding(false));
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not write the IK of " + name + ": " + ex);
                    }
                }

                if (ok && e.Frames > 0 && chara != null)
                {
                    int oldRate = Time.captureFramerate;
                    Time.captureFramerate = SequenceRate;
                    chara.animeSpeed = 1f;
                    for (int f = 1; f <= e.Frames; f++)
                    {
                        for (int k = 0; k < e.Step; k++)
                        {
                            yield return null;
                        }
                        yield return new WaitForEndOfFrame();
                        try
                        {
                            Capture(chara, e, Path.Combine(outDir, name + ".f" + f.ToString("00")), f);
                        }
                        catch (Exception ex)
                        {
                            Logger.LogError("Reference capture: failed on " + name + " frame " + f + ": " + ex);
                        }
                    }
                    chara.animeSpeed = 0f;
                    Time.captureFramerate = oldRate;
                }

                // The next case starts without IK or FK.
                if (chara != null && (e.Ik || e.Fk))
                {
                    try
                    {
                        chara.ActiveKinematicMode(OICharInfo.KinematicMode.IK, false, true);
                        chara.ActiveKinematicMode(OICharInfo.KinematicMode.FK, false, true);
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not switch IK and FK off after " + name + ": " + ex);
                    }
                }

                float took = Time.realtimeSinceStartup - began;
                index.Append(name).Append(ok ? "\tok\t" : "\tfailed\t").Append(F(took)).Append(" s\t").Append(e.Line).Append("\n");
                Logger.LogMessage("Reference capture: " + (n + 1) + "/" + entries.Count + " " + name);
            }

            if (chara != null)
            {
                studio.treeNodeCtrl.DeleteNode(chara.treeNodeObject);
                for (int i = 0; i < 5; i++)
                {
                    yield return null;
                }
            }
            if (saved != null)
            {
                // Leave the scene without the batch's map and its settings.
                if (mapNow != null)
                {
                    try
                    {
                        studio.AddMap(-1, false, false, true);
                    }
                    catch (Exception ex)
                    {
                        Logger.LogError("Reference capture: could not unload the map: " + ex);
                    }
                    for (int i = 0; i < 30; i++)
                    {
                        yield return null;
                    }
                }
                try
                {
                    RestoreScene(studio, saved);
                }
                catch (Exception ex)
                {
                    Logger.LogError("Reference capture: could not restore the scene: " + ex);
                }
            }
            float total = Time.realtimeSinceStartup - startedAll;
            index.Append("# ").Append(done).Append(" of ").Append(entries.Count).Append(" cases in ").Append(F(total)).Append(" s\n");
            if (sub.Length > 0)
            {
                File.WriteAllText(Path.Combine(outDir, "index.txt"), index.ToString(), new UTF8Encoding(false));
            }
            Logger.LogMessage("Reference capture: saved " + done + " of " + entries.Count + " to " + outDir + " in " + (int)total + " s");
            running = false;
        }

        private static Saved SaveScene(Studio.Studio studio)
        {
            Saved s = new Saved();
            s.fog = RenderSettings.fog;
            s.fogMode = RenderSettings.fogMode;
            s.fogColor = RenderSettings.fogColor;
            s.fogStart = RenderSettings.fogStartDistance;
            s.fogEnd = RenderSettings.fogEndDistance;
            AmplifyColorEffect ace = Camera.main.GetComponent<AmplifyColorEffect>();
            if (ace != null)
            {
                s.lut = ace.LutTexture;
                s.lutBlend = ace.LutBlendTexture;
                s.blend = ace.BlendAmount;
            }
            s.sun = studio.sceneInfo.sunLightType;
            s.vanish = studio.cameraCtrl.isConfigVanish;
            return s;
        }

        private static void RestoreScene(Studio.Studio studio, Saved s)
        {
            RenderSettings.fog = s.fog;
            RenderSettings.fogMode = s.fogMode;
            RenderSettings.fogColor = s.fogColor;
            RenderSettings.fogStartDistance = s.fogStart;
            RenderSettings.fogEndDistance = s.fogEnd;
            AmplifyColorEffect ace = Camera.main.GetComponent<AmplifyColorEffect>();
            if (ace != null)
            {
                ace.LutTexture = s.lut;
                ace.LutBlendTexture = s.lutBlend;
                ace.BlendAmount = s.blend;
            }
            studio.sceneInfo.sunLightType = s.sun;
            studio.cameraCtrl.isConfigVanish = s.vanish;
        }

        // A map of Studio's list by number, or by the file name of its
        // scene bundle ("myroom" for map/scene/myroom.unity3d). The lowest
        // number wins, which is the game's own entry.
        private static int FindMap(string name)
        {
            int number;
            Dictionary<int, Info.MapLoadInfo> maps = Singleton<Info>.Instance.dicMapLoadInfo;
            if (int.TryParse(name, out number))
            {
                return maps.ContainsKey(number) ? number : -1;
            }
            int found = -1;
            foreach (KeyValuePair<int, Info.MapLoadInfo> m in maps)
            {
                string file = Path.GetFileNameWithoutExtension(m.Value.bundlePath ?? "");
                if (string.Equals(file, name, StringComparison.OrdinalIgnoreCase) && (found < 0 || m.Key < found))
                {
                    found = m.Key;
                }
            }
            return found;
        }

        private class ViewSpec
        {
            // "body", "face", "back", "side", "zoom" or "cam".
            public string Kind;
            public string Name;
            public string Bone;
            public float Half;
            public float Yaw;
            public float Pitch;
            public Vector3 From;
            public Vector3 To;
            public float Fov;
        }

        private class Entry
        {
            // The list line as written.
            public string Line = "";
            // Card file, or null for no character.
            public string Path;
            // Outfit to wear, or -1 for the one the card loads with.
            public int Outfit = -1;
            public string Label = "";
            public List<ViewSpec> Views = new List<ViewSpec>();
            public int Width = DefaultWidth;
            public int Height = DefaultHeight;
            public int Msaa = 8;
            public bool HasPose;
            // Group, category and number; number -1 is the maker's pose.
            public int[] Pose = new int[3];
            public float Time;
            public int Frames;
            public int Step = 1;
            public List<KeyValuePair<int, int>> Clothes = new List<KeyValuePair<int, int>>();
            public string Map;
            public int Sun;
            public int Settle = SettleFrames;
            public bool PlainOnly;
            public bool Ik;
            // Target number, then the move and the turn.
            public List<KeyValuePair<int, Vector3[]>> IkMoves = new List<KeyValuePair<int, Vector3[]>>();
            public bool Fk;
            public List<KeyValuePair<string, Vector3>> FkTurns = new List<KeyValuePair<string, Vector3>>();
        }

        private static readonly string[] SunNames = { "day", "evening", "night" };

        private static string CaseName(Entry e)
        {
            string name;
            if (e.Path != null)
            {
                name = Path.GetFileNameWithoutExtension(e.Path);
            }
            else
            {
                name = "map." + (e.Map ?? "none");
            }
            if (e.Outfit >= 0)
            {
                name += ".outfit" + e.Outfit;
            }
            if (e.Path != null && e.Map != null)
            {
                name += "." + e.Map;
            }
            if (e.Map != null)
            {
                name += "." + SunNames[e.Sun];
            }
            if (e.Label.Length > 0)
            {
                name += "." + e.Label;
            }
            return name;
        }

        private static float Num(string text)
        {
            return float.Parse(text.Trim(), CultureInfo.InvariantCulture);
        }

        private static Vector3 Point(string text)
        {
            string[] p = text.Split(',');
            return new Vector3(Num(p[0]), Num(p[1]), Num(p[2]));
        }

        private static readonly string[][] ClothesNames =
        {
            new[] { "top" }, new[] { "bot", "bottom" }, new[] { "bra" }, new[] { "shorts" }, new[] { "gloves" },
            new[] { "panst", "pantyhose" }, new[] { "socks" }, new[] { "shoes_inner", "indoor" }, new[] { "shoes_outer", "outdoor" },
        };

        // Reads one field of a list line into the entry.
        private static void Field(Entry e, string field)
        {
            int n;
            if (int.TryParse(field, out n))
            {
                e.Outfit = n;
                return;
            }
            if (field == "plain")
            {
                e.PlainOnly = true;
                return;
            }
            int eq = field.IndexOf('=');
            if (eq < 0)
            {
                throw new FormatException("unknown field " + field);
            }
            string key = field.Substring(0, eq).Trim();
            string value = field.Substring(eq + 1).Trim();
            string[] parts = value.Split(':');
            switch (key)
            {
                case "outfit":
                    e.Outfit = int.Parse(value);
                    break;
                case "name":
                    e.Label = value;
                    break;
                case "view":
                    foreach (string v in value.Split(','))
                    {
                        string kind = v.Trim();
                        if (kind != "body" && kind != "face" && kind != "back" && kind != "side")
                        {
                            throw new FormatException("unknown view " + kind);
                        }
                        e.Views.Add(new ViewSpec { Kind = kind, Name = kind });
                    }
                    break;
                case "zoom":
                    e.Views.Add(new ViewSpec
                    {
                        Kind = "zoom",
                        Name = parts[0],
                        Bone = parts[1],
                        Half = Num(parts[2]),
                        Yaw = parts.Length > 3 ? Num(parts[3]) : 0f,
                        Pitch = parts.Length > 4 ? Num(parts[4]) : 0f,
                    });
                    break;
                case "cam":
                    e.Views.Add(new ViewSpec
                    {
                        Kind = "cam",
                        Name = parts[0],
                        From = Point(parts[1]),
                        To = Point(parts[2]),
                        Fov = parts.Length > 3 ? Num(parts[3]) : 0f,
                    });
                    break;
                case "res":
                    string[] wh = value.Split('x');
                    e.Width = int.Parse(wh[0]);
                    e.Height = int.Parse(wh[1]);
                    break;
                case "msaa":
                    e.Msaa = int.Parse(value);
                    break;
                case "pose":
                    e.HasPose = true;
                    if (value == "maker")
                    {
                        e.Pose = new[] { 0, 0, -1 };
                    }
                    else
                    {
                        string[] p = value.Split('/');
                        e.Pose = new[] { int.Parse(p[0]), int.Parse(p[1]), int.Parse(p[2]) };
                    }
                    break;
                case "time":
                    e.Time = Num(value);
                    break;
                case "frames":
                    e.Frames = int.Parse(value);
                    break;
                case "step":
                    e.Step = Math.Max(1, int.Parse(value));
                    break;
                case "clothes":
                    foreach (string pair in value.Split(','))
                    {
                        string[] ks = pair.Split(':');
                        string kindName = ks[0].Trim();
                        int kind;
                        if (!int.TryParse(kindName, out kind))
                        {
                            kind = -1;
                            for (int i = 0; i < ClothesNames.Length; i++)
                            {
                                if (Array.IndexOf(ClothesNames[i], kindName) >= 0)
                                {
                                    kind = i;
                                }
                            }
                        }
                        if (kind < 0 || kind >= ClothesNames.Length)
                        {
                            throw new FormatException("unknown clothing " + kindName);
                        }
                        e.Clothes.Add(new KeyValuePair<int, int>(kind, int.Parse(ks[1])));
                    }
                    break;
                case "map":
                    e.Map = value;
                    break;
                case "sun":
                    e.Sun = Array.IndexOf(SunNames, value);
                    if (e.Sun < 0)
                    {
                        throw new FormatException("unknown time of day " + value);
                    }
                    break;
                case "settle":
                    e.Settle = int.Parse(value);
                    break;
                case "ik":
                    e.Ik = true;
                    if (value != "on")
                    {
                        int no;
                        if (!int.TryParse(parts[0], out no))
                        {
                            no = (int)Enum.Parse(typeof(OICharInfo.IKTargetEN), parts[0], true);
                        }
                        Vector3 turn = parts.Length > 2 ? Point(parts[2]) : Vector3.zero;
                        e.IkMoves.Add(new KeyValuePair<int, Vector3[]>(no, new[] { Point(parts[1]), turn }));
                    }
                    break;
                case "fk":
                    e.Fk = true;
                    if (value != "on")
                    {
                        e.FkTurns.Add(new KeyValuePair<string, Vector3>(parts[0], Point(parts[1])));
                    }
                    break;
                default:
                    throw new FormatException("unknown field " + key);
            }
        }

        private List<Entry> ReadList(string listPath)
        {
            string female = Path.Combine(Paths.GameRootPath, "UserData/chara/female");
            List<Entry> entries = new List<Entry>();
            if (File.Exists(listPath))
            {
                foreach (string raw in File.ReadAllLines(listPath))
                {
                    string line = raw.Trim();
                    if (line.Length == 0 || line.StartsWith("#"))
                    {
                        continue;
                    }
                    string[] fields = line.Split('|');
                    Entry e = new Entry();
                    e.Line = line;
                    string card = fields[0].Trim();
                    if (card != "-" && card.Length > 0)
                    {
                        e.Path = Path.IsPathRooted(card) ? card : Path.Combine(female, card);
                    }
                    try
                    {
                        for (int i = 1; i < fields.Length; i++)
                        {
                            string field = fields[i].Trim();
                            if (field.Length > 0)
                            {
                                Field(e, field);
                            }
                        }
                    }
                    catch (Exception ex)
                    {
                        Logger.LogWarning("Reference capture: skipped line \"" + line + "\": " + ex.Message);
                        continue;
                    }
                    if (e.Path == null && e.Map == null)
                    {
                        Logger.LogWarning("Reference capture: skipped line \"" + line + "\": no card and no map");
                        continue;
                    }
                    entries.Add(e);
                }
            }
            if (entries.Count == 0)
            {
                entries.Add(new Entry { Path = Path.Combine(female, "Simple.png"), Line = "Simple.png" });
            }
            return entries;
        }

        private class View
        {
            public string name;
            public Vector3 position;
            public Quaternion rotation;
            public float fov;
            // Where the character's light pointed while this was taken.
            public Vector3 light;
            public bool hasLight;
        }

        // The directional light that lights the layer the character is
        // drawn on; Studio's own one when several do.
        private static Light CharaLight(Renderer[] renderers)
        {
            int layer = LayerMask.NameToLayer("Chara");
            foreach (Renderer r in renderers)
            {
                if (r.enabled)
                {
                    layer = r.gameObject.layer;
                    break;
                }
            }
            Light best = null;
            Light named = null;
            foreach (Light l in FindObjectsOfType<Light>())
            {
                if (!l.isActiveAndEnabled || l.type != LightType.Directional)
                {
                    continue;
                }
                if (l.name == "Directional Chara")
                {
                    named = l;
                }
                if (layer >= 0 && (l.cullingMask & (1 << layer)) != 0 && (best == null || l == named))
                {
                    best = l;
                }
            }
            return best ?? named;
        }

        private static Transform Bone(Transform[] bones, string name)
        {
            foreach (Transform t in bones)
            {
                if (t.name == name)
                {
                    return t;
                }
            }
            return null;
        }

        // The cameras of a case, in the order asked for.
        private List<View> Views(Camera cam, ChaControl cha, Renderer[] renderers, Entry e, float aspect)
        {
            List<View> views = new List<View>();
            List<ViewSpec> specs = new List<ViewSpec>(e.Views);
            if (specs.Count == 0 && cha != null)
            {
                specs.Add(new ViewSpec { Kind = "body", Name = "body" });
                specs.Add(new ViewSpec { Kind = "face", Name = "face" });
            }
            if (specs.Count == 0)
            {
                // A map alone: Studio's own camera, then a look around from
                // eye height where characters are placed.
                View own = new View();
                own.name = "studio";
                own.position = cam.transform.position;
                own.rotation = cam.transform.rotation;
                own.fov = cam.fieldOfView;
                views.Add(own);
                string[] names = { "north", "east", "south", "west" };
                for (int i = 0; i < 4; i++)
                {
                    View v = new View();
                    v.name = names[i];
                    v.position = new Vector3(0f, 1.4f, 0f);
                    v.rotation = Quaternion.Euler(0f, 90f * i, 0f);
                    v.fov = 60f;
                    views.Add(v);
                }
                return views;
            }
            Vector3 forward = cha != null ? cha.transform.forward : Vector3.forward;
            Transform[] bones = cha != null ? cha.objBodyBone.GetComponentsInChildren<Transform>(true) : new Transform[0];
            float fov = cam.fieldOfView;
            foreach (ViewSpec s in specs)
            {
                View v = null;
                if (s.Kind == "cam")
                {
                    v = new View();
                    v.position = s.From;
                    v.rotation = Quaternion.LookRotation(s.To - s.From, Vector3.up);
                    v.fov = s.Fov > 0f ? s.Fov : fov;
                }
                else if (cha == null)
                {
                    Logger.LogWarning("Reference capture: view " + s.Name + " needs a character");
                    continue;
                }
                else if (s.Kind == "body")
                {
                    v = Frame(ExactBounds(renderers), 1.05f, fov, aspect, forward);
                }
                else if (s.Kind == "face")
                {
                    if (cha.objHead == null)
                    {
                        continue;
                    }
                    // Wide enough to keep most of the hair in the picture.
                    v = Frame(ExactBounds(cha.objHead.GetComponentsInChildren<Renderer>(false)), 1.7f, fov, aspect, forward);
                }
                else if (s.Kind == "back" || s.Kind == "side")
                {
                    Vector3 dir = Quaternion.AngleAxis(s.Kind == "back" ? 180f : 90f, Vector3.up) * forward;
                    v = FrameFrom(ExactBounds(renderers), 1.05f, fov, aspect, dir);
                }
                else
                {
                    Vector3 target = Vector3.zero;
                    string[] names = s.Bone.Split('+');
                    bool found = true;
                    foreach (string boneName in names)
                    {
                        Transform t = Bone(bones, boneName.Trim());
                        if (t == null)
                        {
                            Logger.LogWarning("Reference capture: no bone " + boneName + " for view " + s.Name);
                            found = false;
                            break;
                        }
                        target += t.position / names.Length;
                    }
                    if (!found)
                    {
                        continue;
                    }
                    Vector3 dir = Quaternion.AngleAxis(s.Yaw, Vector3.up) * forward;
                    dir = Quaternion.AngleAxis(-s.Pitch, Vector3.Cross(Vector3.up, dir)) * dir;
                    v = new View();
                    v.position = target + dir * (s.Half / Mathf.Tan(fov * 0.5f * Mathf.Deg2Rad));
                    v.rotation = Quaternion.LookRotation(-dir, Vector3.up);
                }
                v.name = s.Name;
                if (v.fov <= 0f)
                {
                    v.fov = fov;
                }
                views.Add(v);
            }
            return views;
        }

        private void Capture(OCICharFemale chara, Entry e, string outBase, int frame)
        {
            Camera cam = Camera.main;
            ChaControl cha = chara != null ? chara.charInfo : null;
            Renderer[] renderers = cha != null ? cha.GetComponentsInChildren<Renderer>(false) : new Renderer[0];

            // Remember what gets changed, so Studio is left as it was.
            Vector3 oldPos = cam.transform.position;
            Quaternion oldRot = cam.transform.rotation;
            float oldFov = cam.fieldOfView;
            CameraClearFlags oldFlags = cam.clearFlags;
            Color oldBack = cam.backgroundColor;

            float aspect = (float)e.Width / e.Height;
            List<View> views = Views(cam, cha, renderers, e, aspect);
            // A sequence keeps its first picture only, to stay quick.
            if (frame > 0 && views.Count > 1)
            {
                views.RemoveRange(1, views.Count - 1);
            }

            if (e.Map == null)
            {
                // A map keeps its sky; without one the back is one colour.
                cam.clearFlags = CameraClearFlags.SolidColor;
                cam.backgroundColor = new Color(0.93f, 0.93f, 0.95f, 1f);
            }
            List<Behaviour> effects = ImageEffects(cam);
            Light charaLight = CharaLight(renderers);
            try
            {
                foreach (View v in views)
                {
                    cam.transform.position = v.position;
                    cam.transform.rotation = v.rotation;
                    cam.fieldOfView = v.fov;
                    if (charaLight != null)
                    {
                        v.light = charaLight.transform.forward;
                        v.hasLight = true;
                    }
                    if (!e.PlainOnly && frame == 0)
                    {
                        File.WriteAllBytes(outBase + "." + v.name + ".studio.png", Shot(cam, e));
                    }
                    foreach (Behaviour b in effects)
                    {
                        b.enabled = false;
                    }
                    File.WriteAllBytes(outBase + "." + v.name + ".plain.png", Shot(cam, e));
                    foreach (Behaviour b in effects)
                    {
                        b.enabled = true;
                    }
                }
                cam.fieldOfView = oldFov;
                File.WriteAllText(outBase + ".json", Describe(cam, cha, chara, e, frame, charaLight, renderers, effects, aspect, views), new UTF8Encoding(false));
            }
            finally
            {
                foreach (Behaviour b in effects)
                {
                    b.enabled = true;
                }
                cam.transform.position = oldPos;
                cam.transform.rotation = oldRot;
                cam.fieldOfView = oldFov;
                cam.clearFlags = oldFlags;
                cam.backgroundColor = oldBack;
            }
        }

        // A camera in front of the character that fits the box, with a margin.
        private static View Frame(Bounds box, float margin, float fov, float aspect, Vector3 forward)
        {
            float half = Mathf.Max(box.extents.y, box.extents.x / aspect) * margin;
            float dist = half / Mathf.Tan(fov * 0.5f * Mathf.Deg2Rad) + box.extents.z;
            View v = new View();
            v.position = box.center + forward * dist;
            v.rotation = Quaternion.LookRotation(-forward, Vector3.up);
            return v;
        }

        // The same from any side: the box may be wide either way.
        private static View FrameFrom(Bounds box, float margin, float fov, float aspect, Vector3 dir)
        {
            float wide = Mathf.Max(box.extents.x, box.extents.z);
            float half = Mathf.Max(box.extents.y, wide / aspect) * margin;
            float dist = half / Mathf.Tan(fov * 0.5f * Mathf.Deg2Rad) + wide;
            View v = new View();
            v.position = box.center + dir * dist;
            v.rotation = Quaternion.LookRotation(-dir, Vector3.up);
            return v;
        }

        // Bounds of the drawn surfaces as posed. Renderer.bounds is far too
        // loose for skinned meshes, so those are baked and measured.
        private static Bounds ExactBounds(Renderer[] renderers)
        {
            Vector3 lo = new Vector3(float.MaxValue, float.MaxValue, float.MaxValue);
            Vector3 hi = new Vector3(float.MinValue, float.MinValue, float.MinValue);
            Mesh baked = new Mesh();
            foreach (Renderer r in renderers)
            {
                if (!r.enabled)
                {
                    continue;
                }
                SkinnedMeshRenderer skinned = r as SkinnedMeshRenderer;
                if (skinned != null && skinned.sharedMesh != null)
                {
                    skinned.BakeMesh(baked);
                    // Baked vertices already include the scale.
                    Matrix4x4 m = Matrix4x4.TRS(r.transform.position, r.transform.rotation, Vector3.one);
                    foreach (Vector3 p in baked.vertices)
                    {
                        Vector3 w = m.MultiplyPoint3x4(p);
                        lo = Vector3.Min(lo, w);
                        hi = Vector3.Max(hi, w);
                    }
                }
                else
                {
                    lo = Vector3.Min(lo, r.bounds.min);
                    hi = Vector3.Max(hi, r.bounds.max);
                }
            }
            Destroy(baked);
            Bounds box = new Bounds();
            if (lo.x <= hi.x)
            {
                box.SetMinMax(lo, hi);
            }
            return box;
        }

        // Enabled components on the camera that post-process the picture:
        // those with OnRenderImage, and Amplify Occlusion, which works from a
        // command buffer set up in OnPreRender and removed in OnDisable.
        private static List<Behaviour> ImageEffects(Camera cam)
        {
            List<Behaviour> found = new List<Behaviour>();
            foreach (Behaviour b in cam.GetComponents<Behaviour>())
            {
                if (b == null || !b.enabled)
                {
                    continue;
                }
                for (Type t = b.GetType(); t != null && t != typeof(MonoBehaviour); t = t.BaseType)
                {
                    if (t.Name.StartsWith("AmplifyOcclusion"))
                    {
                        found.Add(b);
                        break;
                    }
                    if (t.GetMethod("OnRenderImage", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly) != null)
                    {
                        found.Add(b);
                        break;
                    }
                }
            }
            return found;
        }

        private static byte[] Shot(Camera cam, Entry e)
        {
            RenderTexture rt = new RenderTexture(e.Width, e.Height, 24, RenderTextureFormat.ARGB32);
            rt.antiAliasing = e.Msaa;
            RenderTexture oldTarget = cam.targetTexture;
            RenderTexture oldActive = RenderTexture.active;
            Texture2D tex = new Texture2D(e.Width, e.Height, TextureFormat.RGB24, false);
            try
            {
                cam.targetTexture = rt;
                cam.Render();
                RenderTexture.active = rt;
                tex.ReadPixels(new Rect(0, 0, e.Width, e.Height), 0, 0);
                tex.Apply();
                return tex.EncodeToPNG();
            }
            finally
            {
                cam.targetTexture = oldTarget;
                cam.ResetAspect();
                RenderTexture.active = oldActive;
                Destroy(tex);
                rt.Release();
                Destroy(rt);
            }
        }

        private static string Describe(Camera cam, ChaControl cha, OCICharFemale chara, Entry e, int frame, Light charaLight, Renderer[] renderers, List<Behaviour> effects, float aspect, List<View> views)
        {
            SceneInfo scene = Singleton<Studio.Studio>.Instance.sceneInfo;
            StringBuilder s = new StringBuilder(1 << 20);
            s.Append("{\n");
            s.Append("\"card\": ").Append(Str(e.Path ?? "")).Append(",\n");
            s.Append("\"unity\": ").Append(Str(Application.unityVersion)).Append(",\n");
            s.Append("\"color_space\": ").Append(Str(QualitySettings.activeColorSpace.ToString())).Append(",\n");
            s.Append("\"size\": [").Append(e.Width).Append(", ").Append(e.Height).Append("],\n");

            // What the list line asked for.
            s.Append("\"case\": {");
            s.Append("\"line\": ").Append(Str(e.Line));
            s.Append(", \"label\": ").Append(Str(e.Label));
            s.Append(", \"outfit\": ").Append(e.Outfit);
            s.Append(", \"msaa\": ").Append(e.Msaa);
            s.Append(", \"settle_frames\": ").Append(e.Settle);
            s.Append(", \"studio_pictures\": ").Append(B(!e.PlainOnly && frame == 0));
            if (e.HasPose)
            {
                s.Append(", \"pose\": {\"group\": ").Append(e.Pose[0]).Append(", \"category\": ").Append(e.Pose[1]);
                s.Append(", \"number\": ").Append(e.Pose[2]).Append(", \"time\": ").Append(F(e.Time)).Append("}");
            }
            if (e.Frames > 0)
            {
                s.Append(", \"sequence\": {\"frame\": ").Append(frame).Append(", \"frames\": ").Append(e.Frames);
                s.Append(", \"step\": ").Append(e.Step).Append(", \"rate\": ").Append(SequenceRate).Append("}");
            }
            if (e.Map != null)
            {
                s.Append(", \"map\": ").Append(Str(e.Map)).Append(", \"sun\": ").Append(Str(SunNames[e.Sun]));
            }
            s.Append("},\n");

            s.Append("\"camera\": {");
            s.Append("\"fov\": ").Append(F(cam.fieldOfView));
            s.Append(", \"aspect\": ").Append(F(aspect));
            s.Append(", \"near\": ").Append(F(cam.nearClipPlane));
            s.Append(", \"far\": ").Append(F(cam.farClipPlane));
            s.Append(", \"hdr\": ").Append(B(cam.allowHDR));
            s.Append(", \"path\": ").Append(Str(cam.actualRenderingPath.ToString()));
            s.Append(", \"background\": ").Append(C(cam.backgroundColor));
            s.Append(", \"clear\": ").Append(Str(cam.clearFlags.ToString()));
            s.Append(", \"culling_mask\": ").Append(cam.cullingMask);
            s.Append(", \"screen_msaa\": ").Append(cam.allowMSAA ? QualitySettings.antiAliasing : 0);
            s.Append("},\n");

            // One picture pair per view: <name>.<view>.studio.png and .plain.png.
            // A view's own "fov" and "light_direction" replace the scene's.
            s.Append("\"views\": [");
            for (int i = 0; i < views.Count; i++)
            {
                s.Append(i == 0 ? "" : ", ");
                s.Append("{\"name\": ").Append(Str(views[i].name));
                s.Append(", \"position\": ").Append(V(views[i].position));
                s.Append(", \"rotation\": ").Append(Q(views[i].rotation));
                s.Append(", \"fov\": ").Append(F(views[i].fov));
                if (views[i].hasLight)
                {
                    s.Append(", \"light_direction\": ").Append(V(views[i].light));
                }
                s.Append("}");
            }
            s.Append("],\n");

            if (cha != null)
            {
                ChaFileStatus st = cha.fileStatus;
                s.Append("\"status\": {");
                s.Append("\"coordinate\": ").Append(st.coordinateType);
                s.Append(", \"shoes\": ").Append(st.shoesType);
                s.Append(", \"clothes_state\": [");
                for (int i = 0; i < st.clothesState.Length; i++)
                {
                    s.Append(i == 0 ? "" : ", ").Append(st.clothesState[i]);
                }
                s.Append("]");
                s.Append(", \"eyes_pattern\": ").Append(st.eyesPtn);
                s.Append(", \"eyes_open_max\": ").Append(F(st.eyesOpenMax));
                s.Append(", \"eyes_blink\": ").Append(B(st.eyesBlink));
                s.Append(", \"eyebrow_pattern\": ").Append(st.eyebrowPtn);
                s.Append(", \"mouth_pattern\": ").Append(st.mouthPtn);
                s.Append(", \"mouth_open_max\": ").Append(F(st.mouthOpenMax));
                s.Append(", \"eyes_look_pattern\": ").Append(st.eyesLookPtn);
                s.Append(", \"neck_look_pattern\": ").Append(st.neckLookPtn);
                s.Append(", \"cheek_red\": ").Append(F(st.hohoAkaRate));
                s.Append(", \"skin_gloss\": ").Append(F(st.skinTuyaRate));
                s.Append(", \"hide_eye_highlight\": ").Append(B(st.hideEyesHighlight));
                s.Append("},\n");
                string animation;
                try
                {
                    animation = Animation(cha, chara);
                }
                catch (Exception ex)
                {
                    animation = "{\"error\": " + Str(ex.Message) + "}";
                }
                s.Append("\"animation\": ").Append(animation).Append(",\n");
            }

            s.Append("\"effects_on_camera\": [");
            for (int i = 0; i < effects.Count; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(Str(effects[i].GetType().FullName));
            }
            s.Append("],\n");

            // The public fields of each effect, so that the comparison uses
            // the values of this scene and not the ones Studio starts with.
            s.Append("\"effect_params\": [");
            for (int i = 0; i < effects.Count; i++)
            {
                s.Append(i == 0 ? "\n" : ",\n").Append(EffectFields(effects[i]));
            }
            s.Append("],\n");

            s.Append("\"studio\": {");
            s.Append("\"ace_no\": ").Append(scene.aceNo);
            s.Append(", \"ace_blend\": ").Append(F(scene.aceBlend));
            s.Append(", \"ao\": ").Append(B(scene.enableAOE));
            s.Append(", \"bloom\": ").Append(B(scene.enableBloom));
            s.Append(", \"depth_of_field\": ").Append(B(scene.enableDepth));
            s.Append(", \"vignette\": ").Append(B(scene.enableVignette));
            s.Append(", \"fog\": ").Append(B(scene.enableFog));
            s.Append(", \"sun_shafts\": ").Append(B(scene.enableSunShafts));
            s.Append(", \"shadow\": ").Append(B(scene.enableShadow));
            s.Append(", \"face_normal\": ").Append(B(scene.faceNormal));
            s.Append(", \"face_shadow\": ").Append(B(scene.faceShadow));
            s.Append(", \"line_color_g\": ").Append(F(scene.lineColorG));
            s.Append(", \"line_width_g\": ").Append(F(scene.lineWidthG));
            s.Append(", \"ramp_g\": ").Append(scene.rampG);
            s.Append(", \"ambient_shadow\": ").Append(C(scene.ambientShadow));
            s.Append(", \"ambient_shadow_g\": ").Append(F(scene.ambientShadowG));
            s.Append(", \"chara_light_color\": ").Append(C(scene.charaLight.color));
            s.Append(", \"chara_light_intensity\": ").Append(F(scene.charaLight.intensity));
            s.Append(", \"chara_light_rot\": [").Append(F(scene.charaLight.rot[0])).Append(", ").Append(F(scene.charaLight.rot[1])).Append("]");
            s.Append(", \"chara_light_shadow\": ").Append(B(scene.charaLight.shadow));
            s.Append(", \"anime_speed\": ").Append(F(chara != null ? chara.animeSpeed : 0f));
            s.Append(", \"map\": ").Append(scene.map);
            s.Append(", \"sun_light_type\": ").Append(scene.sunLightType);
            s.Append(", \"map_option\": ").Append(B(scene.mapOption));
            s.Append(", \"map_light_color\": ").Append(C(scene.mapLight.color));
            s.Append(", \"map_light_intensity\": ").Append(F(scene.mapLight.intensity));
            s.Append(", \"map_light_rot\": [").Append(F(scene.mapLight.rot[0])).Append(", ").Append(F(scene.mapLight.rot[1])).Append("]");
            s.Append(", \"map_light_shadow\": ").Append(B(scene.mapLight.shadow));
            s.Append(", \"map_light_type\": ").Append(Str(scene.mapLight.type.ToString()));
            s.Append("},\n");

            Texture ramp = Shader.GetGlobalTexture("_RampG");
            s.Append("\"globals\": {");
            s.Append("\"_ambientshadowG\": ").Append(C(Shader.GetGlobalColor("_ambientshadowG")));
            s.Append(", \"_LineColorG\": ").Append(C(Shader.GetGlobalColor("_LineColorG")));
            s.Append(", \"_linewidthG\": ").Append(F(Shader.GetGlobalFloat("_linewidthG")));
            s.Append(", \"_RampG\": ").Append(Str(ramp != null ? ramp.name : ""));
            s.Append(", \"ambient_mode\": ").Append(Str(RenderSettings.ambientMode.ToString()));
            s.Append(", \"ambient_light\": ").Append(C(RenderSettings.ambientLight));
            s.Append(", \"ambient_intensity\": ").Append(F(RenderSettings.ambientIntensity));
            // What shaders with Unity's own lighting read: the ambient probe
            // (nine coefficients per colour channel) and the default
            // reflection.
            UnityEngine.Rendering.SphericalHarmonicsL2 probe = RenderSettings.ambientProbe;
            s.Append(", \"ambient_probe\": [");
            for (int ch = 0; ch < 3; ch++)
                for (int k = 0; k < 9; k++)
                    s.Append(ch + k > 0 ? ", " : "").Append(F(probe[ch, k]));
            s.Append("]");
            Cubemap reflection = RenderSettings.customReflection;
            s.Append(", \"reflection_mode\": ").Append(Str(RenderSettings.defaultReflectionMode.ToString()));
            s.Append(", \"reflection_intensity\": ").Append(F(RenderSettings.reflectionIntensity));
            s.Append(", \"custom_reflection\": ").Append(Str(reflection != null ? reflection.name : ""));
            s.Append(", \"skybox\": ").Append(Str(RenderSettings.skybox != null ? RenderSettings.skybox.name : ""));
            s.Append(", \"skybox_shader\": ").Append(Str(RenderSettings.skybox != null && RenderSettings.skybox.shader != null ? RenderSettings.skybox.shader.name : ""));
            s.Append(", \"ambient_sky\": ").Append(C(RenderSettings.ambientSkyColor));
            s.Append(", \"ambient_equator\": ").Append(C(RenderSettings.ambientEquatorColor));
            s.Append(", \"ambient_ground\": ").Append(C(RenderSettings.ambientGroundColor));
            s.Append(", \"sun\": ").Append(Str(RenderSettings.sun != null ? RenderSettings.sun.name : ""));
            s.Append(", \"fog\": ").Append(B(RenderSettings.fog));
            s.Append(", \"fog_mode\": ").Append(Str(RenderSettings.fogMode.ToString()));
            s.Append(", \"fog_color\": ").Append(C(RenderSettings.fogColor));
            s.Append(", \"fog_start\": ").Append(F(RenderSettings.fogStartDistance));
            s.Append(", \"fog_end\": ").Append(F(RenderSettings.fogEndDistance));
            s.Append(", \"fog_density\": ").Append(F(RenderSettings.fogDensity));
            // The colour filter that is on the camera now, whoever set it.
            AmplifyColorEffect ace = cam.GetComponent<AmplifyColorEffect>();
            if (ace != null)
            {
                s.Append(", \"ace_enabled\": ").Append(B(ace.enabled));
                s.Append(", \"ace_lut\": ").Append(Str(ace.LutTexture != null ? ace.LutTexture.name : ""));
                s.Append(", \"ace_lut_blend\": ").Append(Str(ace.LutBlendTexture != null ? ace.LutBlendTexture.name : ""));
                s.Append(", \"ace_blend_amount\": ").Append(F(ace.BlendAmount));
            }
            s.Append("},\n");

            s.Append("\"quality\": {");
            s.Append("\"level\": ").Append(QualitySettings.GetQualityLevel());
            s.Append(", \"anti_aliasing\": ").Append(QualitySettings.antiAliasing);
            s.Append(", \"shadows\": ").Append(Str(QualitySettings.shadows.ToString()));
            s.Append(", \"shadow_resolution\": ").Append(Str(QualitySettings.shadowResolution.ToString()));
            s.Append(", \"shadow_projection\": ").Append(Str(QualitySettings.shadowProjection.ToString()));
            s.Append(", \"shadow_distance\": ").Append(F(QualitySettings.shadowDistance));
            s.Append(", \"shadow_near_plane_offset\": ").Append(F(QualitySettings.shadowNearPlaneOffset));
            s.Append(", \"shadow_cascades\": ").Append(QualitySettings.shadowCascades);
            s.Append(", \"shadow_cascade2_split\": ").Append(F(QualitySettings.shadowCascade2Split));
            s.Append(", \"shadow_cascade4_split\": ").Append(V(QualitySettings.shadowCascade4Split));
            s.Append(", \"pixel_light_count\": ").Append(QualitySettings.pixelLightCount);
            s.Append(", \"anisotropic\": ").Append(Str(QualitySettings.anisotropicFiltering.ToString()));
            s.Append(", \"texture_limit\": ").Append(QualitySettings.masterTextureLimit);
            s.Append(", \"lod_bias\": ").Append(F(QualitySettings.lodBias));
            s.Append("},\n");

            // Layer names by number, for the lights' culling masks.
            s.Append("\"layers\": [");
            for (int i = 0; i < 32; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(Str(LayerMask.LayerToName(i)));
            }
            s.Append("],\n");

            // The character's light first: with a map there are several
            // directional lights, and readers take the first.
            List<Light> lights = new List<Light>();
            if (charaLight != null)
            {
                lights.Add(charaLight);
            }
            foreach (Light l in FindObjectsOfType<Light>())
            {
                if (l.isActiveAndEnabled && l != charaLight)
                {
                    lights.Add(l);
                }
            }
            s.Append("\"lights\": [");
            bool first = true;
            foreach (Light l in lights)
            {
                s.Append(first ? "\n" : ",\n");
                first = false;
                s.Append("{\"name\": ").Append(Str(l.name));
                s.Append(", \"type\": ").Append(Str(l.type.ToString()));
                s.Append(", \"color\": ").Append(C(l.color));
                s.Append(", \"intensity\": ").Append(F(l.intensity));
                s.Append(", \"direction\": ").Append(V(l.transform.forward));
                s.Append(", \"position\": ").Append(V(l.transform.position));
                s.Append(", \"shadows\": ").Append(Str(l.shadows.ToString()));
                s.Append(", \"shadow_strength\": ").Append(F(l.shadowStrength));
                s.Append(", \"culling_mask\": ").Append(l.cullingMask);
                s.Append(", \"layer\": ").Append(l.gameObject.layer);
                s.Append(", \"parent\": ").Append(Str(l.transform.parent != null ? l.transform.parent.name : ""));
                s.Append(", \"range\": ").Append(F(l.range));
                s.Append(", \"spot_angle\": ").Append(F(l.spotAngle));
                s.Append(", \"shadow_bias\": ").Append(F(l.shadowBias));
                s.Append(", \"shadow_normal_bias\": ").Append(F(l.shadowNormalBias));
                s.Append(", \"shadow_near_plane\": ").Append(F(l.shadowNearPlane));
                s.Append(", \"render_mode\": ").Append(Str(l.renderMode.ToString()));
                s.Append(", \"lights_character\": ").Append(B(l == charaLight));
                s.Append("}");
            }
            s.Append("\n]");

            if (Singleton<Map>.IsInstance() && Singleton<Map>.Instance.mapRoot != null)
            {
                string map;
                try
                {
                    map = MapData(cam);
                }
                catch (Exception ex)
                {
                    map = "{\"error\": " + Str(ex.Message) + "}";
                }
                s.Append(",\n\"map\": ").Append(map);
            }
            if (cha == null)
            {
                s.Append("\n}\n");
                return s.ToString();
            }
            s.Append(",\n");

            // Every bone with its parent, so the exact pose and shape can be rebuilt.
            Transform[] bones = cha.objBodyBone.GetComponentsInChildren<Transform>(true);
            Dictionary<Transform, int> index = new Dictionary<Transform, int>();
            for (int i = 0; i < bones.Length; i++)
            {
                index[bones[i]] = i;
            }
            s.Append("\"bones\": [");
            for (int i = 0; i < bones.Length; i++)
            {
                Transform t = bones[i];
                int parent;
                if (t.parent == null || !index.TryGetValue(t.parent, out parent))
                {
                    parent = -1;
                }
                s.Append(i == 0 ? "\n" : ",\n");
                s.Append("{\"name\": ").Append(Str(t.name));
                s.Append(", \"parent\": ").Append(parent);
                s.Append(", \"active\": ").Append(B(t.gameObject.activeInHierarchy));
                s.Append(", \"t\": ").Append(V(t.localPosition));
                s.Append(", \"r\": ").Append(Q(t.localRotation));
                s.Append(", \"s\": ").Append(V(t.localScale));
                s.Append(", \"world\": ").Append(M(t.localToWorldMatrix));
                s.Append("}");
            }
            s.Append("\n],\n");

            s.Append("\"renderers\": [");
            first = true;
            foreach (Renderer r in renderers)
            {
                if (!r.enabled)
                {
                    continue;
                }
                s.Append(first ? "\n" : ",\n");
                first = false;
                s.Append("{\"name\": ").Append(Str(r.name));
                s.Append(", \"kind\": ").Append(Str(r.GetType().Name));
                s.Append(", \"layer\": ").Append(r.gameObject.layer);
                s.Append(", \"cast\": ").Append(Str(r.shadowCastingMode.ToString()));
                s.Append(", \"receive\": ").Append(r.receiveShadows ? "true" : "false");
                SkinnedMeshRenderer skinned = r as SkinnedMeshRenderer;
                if (skinned != null)
                {
                    s.Append(", \"mesh\": ").Append(Str(skinned.sharedMesh != null ? skinned.sharedMesh.name : ""));
                    s.Append(", \"root_bone\": ").Append(Str(skinned.rootBone != null ? skinned.rootBone.name : ""));
                    s.Append(", \"bones\": [");
                    Transform[] skin = skinned.bones ?? new Transform[0];
                    for (int i = 0; i < skin.Length; i++)
                    {
                        s.Append(i == 0 ? "" : ", ").Append(Str(skin[i] != null ? skin[i].name : ""));
                    }
                    s.Append("]");
                }
                if (skinned != null && skinned.sharedMesh != null && skinned.sharedMesh.blendShapeCount > 0)
                {
                    // Blend shapes in use, as [index, weight in percent].
                    s.Append(", \"shapes\": [");
                    bool firstShape = true;
                    for (int i = 0; i < skinned.sharedMesh.blendShapeCount; i++)
                    {
                        float w = skinned.GetBlendShapeWeight(i);
                        if (w == 0f)
                        {
                            continue;
                        }
                        s.Append(firstShape ? "" : ", ").Append("[").Append(i).Append(", ").Append(F(w)).Append("]");
                        firstShape = false;
                    }
                    s.Append("]");
                }
                if (skinned != null && skinned.sharedMesh != null && r.name == "cf_O_canine")
                {
                    // The fang mesh as Edit Fangs left it, in mesh space.
                    s.Append(", \"vertices\": [");
                    Vector3[] vs = skinned.sharedMesh.vertices;
                    for (int i = 0; i < vs.Length; i++)
                    {
                        s.Append(i == 0 ? "" : ", ").Append("[").Append(F(vs[i].x)).Append(", ")
                            .Append(F(vs[i].y)).Append(", ").Append(F(vs[i].z)).Append("]");
                    }
                    s.Append("]");
                }
                s.Append(", \"materials\": [");
                Material[] mats = r.sharedMaterials;
                for (int i = 0; i < mats.Length; i++)
                {
                    Material m = mats[i];
                    s.Append(i == 0 ? "" : ", ");
                    s.Append("{\"name\": ").Append(Str(m != null ? m.name : ""));
                    s.Append(", \"shader\": ").Append(Str(m != null && m.shader != null ? m.shader.name : ""));
                    s.Append(", \"queue\": ").Append(m != null ? m.renderQueue : 0);
                    if (m != null && m.HasProperty("_MainTex"))
                    {
                        Vector2 off = m.GetTextureOffset("_MainTex");
                        Vector2 scale = m.GetTextureScale("_MainTex");
                        s.Append(", \"main_tex_st\": [").Append(F(scale.x)).Append(", ").Append(F(scale.y)).Append(", ").Append(F(off.x)).Append(", ").Append(F(off.y)).Append("]");
                    }
                    if (m != null)
                    {
                        Properties(s, m);
                    }
                    s.Append("}");
                }
                s.Append("]}");
            }
            s.Append("\n]\n}\n");
            return s.ToString();
        }

        // The state the body's Animator is in, the clips it blends and the
        // numbers it was given.
        private static string Animation(ChaControl cha, OCICharFemale chara)
        {
            StringBuilder s = new StringBuilder("{");
            Animator a = cha.animBody;
            if (a == null || a.runtimeAnimatorController == null)
            {
                return "{}";
            }
            AnimatorStateInfo st = a.GetCurrentAnimatorStateInfo(0);
            s.Append("\"controller\": ").Append(Str(a.runtimeAnimatorController.name));
            s.Append(", \"group\": ").Append(chara.oiCharInfo.animeInfo.group);
            s.Append(", \"category\": ").Append(chara.oiCharInfo.animeInfo.category);
            s.Append(", \"number\": ").Append(chara.oiCharInfo.animeInfo.no);
            s.Append(", \"state_hash\": ").Append(st.shortNameHash);
            s.Append(", \"normalized_time\": ").Append(F(st.normalizedTime));
            s.Append(", \"length\": ").Append(F(st.length));
            s.Append(", \"state_speed\": ").Append(F(st.speed));
            s.Append(", \"speed_multiplier\": ").Append(F(st.speedMultiplier));
            s.Append(", \"loop\": ").Append(B(st.loop));
            s.Append(", \"animator_speed\": ").Append(F(a.speed));
            s.Append(", \"delta_time\": ").Append(F(Time.deltaTime));
            s.Append(", \"force_loop\": ").Append(B(chara.charAnimeCtrl != null && chara.charAnimeCtrl.isForceLoop));
            s.Append(", \"clips\": [");
            AnimatorClipInfo[] clips = a.GetCurrentAnimatorClipInfo(0);
            for (int i = 0; i < clips.Length; i++)
            {
                s.Append(i == 0 ? "" : ", ");
                s.Append("{\"name\": ").Append(Str(clips[i].clip != null ? clips[i].clip.name : ""));
                s.Append(", \"weight\": ").Append(F(clips[i].weight));
                s.Append(", \"length\": ").Append(F(clips[i].clip != null ? clips[i].clip.length : 0f));
                s.Append("}");
            }
            s.Append("], \"parameters\": {");
            bool first = true;
            foreach (AnimatorControllerParameter p in a.parameters)
            {
                if (p.type != AnimatorControllerParameterType.Float)
                {
                    continue;
                }
                s.Append(first ? "" : ", ").Append(Str(p.name)).Append(": ").Append(F(a.GetFloat(p.nameHash)));
                first = false;
            }
            s.Append("}, \"layer_weights\": [");
            for (int i = 0; i < a.layerCount; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(F(a.GetLayerWeight(i)));
            }
            s.Append("]}");
            return s.ToString();
        }

        // The loaded map: its list entry, what each time of day sets, the
        // lightmaps, reflection probes and every renderer with its layer.
        private static string MapData(Camera cam)
        {
            Map map = Singleton<Map>.Instance;
            GameObject root = map.mapRoot;
            StringBuilder s = new StringBuilder("{");
            s.Append("\"number\": ").Append(map.no);
            Info.MapLoadInfo info;
            if (Singleton<Info>.Instance.dicMapLoadInfo.TryGetValue(map.no, out info))
            {
                s.Append(", \"name\": ").Append(Str(info.name ?? ""));
                s.Append(", \"bundle\": ").Append(Str(info.bundlePath ?? ""));
                s.Append(", \"scene\": ").Append(Str(info.fileName ?? ""));
            }
            s.Append(", \"position\": ").Append(V(root.transform.position));
            s.Append(", \"rotation\": ").Append(Q(root.transform.rotation));
            s.Append(", \"sun_type\": ").Append(Str(map.sunType.ToString()));
            s.Append(", \"has_options\": ").Append(B(map.isOption));

            SunLightInfo sun = map.sunLightInfo;
            s.Append(", \"sun_light_info\": ");
            if (sun == null)
            {
                s.Append("null");
            }
            else
            {
                s.Append("{\"target_light\": ").Append(Str(sun.targetLight != null ? sun.targetLight.name : ""));
                s.Append(", \"times\": [");
                SunLightInfo.Info[] infos = sun.infos ?? new SunLightInfo.Info[0];
                for (int i = 0; i < infos.Length; i++)
                {
                    SunLightInfo.Info t = infos[i];
                    s.Append(i == 0 ? "\n" : ",\n");
                    s.Append("{\"type\": ").Append(Str(t.type.ToString()));
                    s.Append(", \"angle\": ").Append(V(t.angle));
                    s.Append(", \"color\": ").Append(C(t.color));
                    s.Append(", \"intensity\": ").Append(F(t.intensity));
                    s.Append(", \"fog\": ").Append(B(t.fogUse));
                    s.Append(", \"fog_color\": ").Append(C(t.fogColor));
                    s.Append(", \"fog_start\": ").Append(F(t.fogStart));
                    s.Append(", \"fog_end\": ").Append(F(t.fogEnd));
                    s.Append(", \"sun_shafts_color\": ").Append(C(t.sunShaftsColor));
                    s.Append(", \"sun_shafts_caster\": ").Append(Str(t.sunShaftsTransform != null ? t.sunShaftsTransform.name : ""));
                    s.Append(", \"lut\": ").Append(Str(t.aceLutTexture != null ? t.aceLutTexture.name : ""));
                    s.Append(", \"lut_blend\": ").Append(Str(t.aceLutBlendTexture != null ? t.aceLutBlendTexture.name : ""));
                    s.Append(", \"lut_blend_amount\": ").Append(F(t.aceBlendAmount));
                    s.Append(", \"back_texture\": ").Append(Str(t.backTexture != null ? t.backTexture.name : ""));
                    s.Append(", \"visible\": [");
                    for (int k = 0; k < t.visibleList.Count; k++)
                    {
                        s.Append(k == 0 ? "" : ", ").Append(Str(t.visibleList[k] != null ? t.visibleList[k].name : ""));
                    }
                    s.Append("]}");
                }
                s.Append("\n]}");
            }

            // The property was renamed between Unity versions.
            LightmapData[] lightmaps = LightmapSettings.lightmaps ?? new LightmapData[0];
            s.Append(", \"lightmaps_mode\": ").Append(Str(LightmapSettings.lightmapsMode.ToString()));
            s.Append(", \"lightmaps\": [");
            for (int i = 0; i < lightmaps.Length; i++)
            {
                Texture color = null;
                Texture dir = null;
                foreach (PropertyInfo p in typeof(LightmapData).GetProperties())
                {
                    if (p.Name == "lightmapColor" || p.Name == "lightmapLight" || p.Name == "lightmapFar")
                    {
                        color = color ?? p.GetValue(lightmaps[i], null) as Texture;
                    }
                    else if (p.Name == "lightmapDir" || p.Name == "lightmapNear")
                    {
                        dir = dir ?? p.GetValue(lightmaps[i], null) as Texture;
                    }
                }
                s.Append(i == 0 ? "" : ", ");
                s.Append("{\"color\": ").Append(Str(color != null ? color.name : ""));
                s.Append(", \"size\": [").Append(color != null ? color.width : 0).Append(", ").Append(color != null ? color.height : 0).Append("]");
                s.Append(", \"dir\": ").Append(Str(dir != null ? dir.name : "")).Append("}");
            }
            s.Append("]");

            s.Append(", \"reflection_probes\": [");
            bool first = true;
            foreach (ReflectionProbe p in root.GetComponentsInChildren<ReflectionProbe>(true))
            {
                s.Append(first ? "" : ", ");
                first = false;
                s.Append("{\"name\": ").Append(Str(p.name));
                s.Append(", \"active\": ").Append(B(p.isActiveAndEnabled));
                s.Append(", \"position\": ").Append(V(p.transform.position));
                s.Append(", \"center\": ").Append(V(p.center));
                s.Append(", \"size\": ").Append(V(p.size));
                s.Append(", \"mode\": ").Append(Str(p.mode.ToString()));
                s.Append(", \"intensity\": ").Append(F(p.intensity));
                s.Append(", \"box_projection\": ").Append(B(p.boxProjection));
                s.Append(", \"importance\": ").Append(p.importance);
                s.Append(", \"texture\": ").Append(Str(p.texture != null ? p.texture.name : "")).Append("}");
            }
            s.Append("]");

            // Other cameras a map brings along.
            s.Append(", \"cameras\": [");
            first = true;
            foreach (Camera c in root.GetComponentsInChildren<Camera>(true))
            {
                s.Append(first ? "" : ", ").Append(Str(c.name));
                first = false;
            }
            s.Append("]");

            Renderer[] renderers = root.GetComponentsInChildren<Renderer>(true);
            Bounds box = new Bounds();
            bool any = false;
            s.Append(", \"renderers\": [");
            first = true;
            foreach (Renderer r in renderers)
            {
                bool shown = r.enabled && r.gameObject.activeInHierarchy;
                if (shown)
                {
                    if (any)
                    {
                        box.Encapsulate(r.bounds);
                    }
                    else
                    {
                        box = r.bounds;
                        any = true;
                    }
                }
                s.Append(first ? "\n" : ",\n");
                first = false;
                s.Append("{\"name\": ").Append(Str(r.name));
                s.Append(", \"kind\": ").Append(Str(r.GetType().Name));
                s.Append(", \"shown\": ").Append(B(shown));
                s.Append(", \"layer\": ").Append(r.gameObject.layer);
                s.Append(", \"static\": ").Append(B(r.gameObject.isStatic));
                s.Append(", \"cast\": ").Append(Str(r.shadowCastingMode.ToString()));
                s.Append(", \"receive\": ").Append(B(r.receiveShadows));
                s.Append(", \"lightmap\": ").Append(r.lightmapIndex);
                Vector4 so = r.lightmapScaleOffset;
                s.Append(", \"lightmap_st\": [").Append(F(so.x)).Append(", ").Append(F(so.y)).Append(", ").Append(F(so.z)).Append(", ").Append(F(so.w)).Append("]");
                s.Append(", \"center\": ").Append(V(r.bounds.center));
                s.Append(", \"extents\": ").Append(V(r.bounds.extents));
                s.Append(", \"materials\": [");
                Material[] mats = r.sharedMaterials;
                for (int i = 0; i < mats.Length; i++)
                {
                    Material m = mats[i];
                    s.Append(i == 0 ? "" : ", ");
                    s.Append("{\"name\": ").Append(Str(m != null ? m.name : ""));
                    s.Append(", \"shader\": ").Append(Str(m != null && m.shader != null ? m.shader.name : ""));
                    s.Append(", \"queue\": ").Append(m != null ? m.renderQueue : 0);
                    s.Append(", \"keywords\": [");
                    string[] kw = m != null && m.shaderKeywords != null ? m.shaderKeywords : new string[0];
                    for (int k = 0; k < kw.Length; k++)
                    {
                        s.Append(k == 0 ? "" : ", ").Append(Str(kw[k]));
                    }
                    s.Append("]}");
                }
                s.Append("]}");
            }
            s.Append("\n]");
            s.Append(", \"bounds_center\": ").Append(V(box.center));
            s.Append(", \"bounds_extents\": ").Append(V(box.extents));
            s.Append("}");
            return s.ToString();
        }

        // Material properties to record, by type. Unity 5.6 cannot list a
        // material's properties at runtime, so these are the properties of
        // the game's shaders and the common modded ones (generated from the
        // shaders' property tables in mm_base, oo_base, co_top_00,
        // ao_face_00, ao_hair_00 and xukmi Vanilla Plus), with those of
        // Unity's Standard shader and IBL_Shader_alpha after them.
        private static readonly string[] Colors = {
            "_Col0", "_Col1", "_Col2", "_Col3", "_Color", "_Color1_2",
            "_Color2", "_Color2_2", "_Color3", "_Color3_2", "_Color4", "_Color5",
            "_Color6", "_Color7", "_CustomAmbient", "_EmissionColor", "_FillColor", "_KKPRimColor",
            "_LineColor", "_OutlineColor", "_ReflectCol", "_ShadowColor", "_SpecularColor", "_overcolor1",
            "_overcolor2", "_overcolor3", "_shadowcolor",
            "_SpecColor", "_rimcolor",
        };

        private static readonly string[] Vectors = {
            "_Clock", "_ClockDisp", "_GlossColor", "_LiquidTiling", "_Patternuv1", "_Patternuv2",
            "_Patternuv3", "_ShadowHSV", "_TileAnimation", "_grad", "_hokuro", "_paint1",
            "_paint2", "_tex4uv", "_tex5uv",
        };

        private static readonly string[] Floats = {
            "_AdjustBackfaceNormals", "_Alpha", "_AlphaOptionCutoff", "_AlphaOptionZWrite", "_AngleSpeed", "_AnotherRampFull",
            "_Blend", "_ColorInverse", "_ColorSort", "_CullOption", "_Cutoff", "_DetailBLineG",
            "_DetailBLineGA", "_DetailNormalMapScale", "_DetailRLineR", "_DisablePointLights", "_DisableShadowedMatcap", "_DisplaceFull",
            "_DisplaceMultiplier", "_DisplaceNormalMultiplier", "_EmissionIntensity", "_EmissionKeepCol", "_EmissionMaskMode", "_EmissionPower",
            "_ExpressionDepth", "_ExpressionSize", "_KKPRimAsDiffuse", "_KKPRimIntensity", "_KKPRimRotateX", "_KKPRimRotateY",
            "_KKPRimSoft", "_LineWidthS", "_NormalMapScale", "_OutlineOn", "_PatternScale1u", "_PatternScale1v",
            "_PatternScale2u", "_PatternScale2v", "_PatternScale3u", "_PatternScale3v", "_ReflBlendDst", "_ReflBlendSrc",
            "_ReflBlendVal", "_ReflectColMix", "_ReflectRotation", "_ReflectionVal", "_Reflective", "_ReflectiveBlend",
            "_ReflectiveMulOrAdd", "_ReflectiveOverlayed", "_RimPower", "_Roughness", "_Saturation", "_ShadowExtend",
            "_ShadowExtendAnother", "_ShrinkVal", "_ShrinkVerticalAdjust", "_SizeSpeed", "_SizeWidth", "_SpeclarHeight",
            "_SpecularDetailNormalScale", "_SpecularHairPower", "_SpecularHeightInvert", "_SpecularIsHighLightsPow", "_SpecularIsHighlights", "_SpecularIsHighlightsRange",
            "_SpecularNormalScale", "_SpecularPower", "_SpecularPowerNail", "_TessBias", "_TessMax", "_TessMin",
            "_TessSmooth", "_Tolerance", "_UseDetailRAsSpecularMap", "_UseDiscard", "_UseKKMetal", "_UseKKPRim",
            "_UseLightColorSpecular", "_UseMatCapReflection", "_UseMeshSpecular", "_UseRampForLights", "_UseRampForShadows", "_UseRampForSpecular",
            "_WhiteOff", "_WireThickness", "_alpha", "_alpha_a", "_alpha_b", "_ambientshadowOFF",
            "_dst", "_exppower", "_isHighLight", "_linetexon", "_liquidbbot", "_liquidbtop",
            "_liquidface", "_liquidfbot", "_liquidftop", "_nip", "_nip_specular", "_nipsize",
            "_notusetexspecular", "_outline", "_patternclamp1", "_patternclamp2", "_patternclamp3", "_patternrotator1",
            "_patternrotator2", "_patternrotator3", "_rimReflectMode", "_rimV", "_rimpower", "_rotation",
            "_src", "_tex1mask", "_transparency", "_yurayura",
            "_BumpScale", "_Cull", "_DstBlend", "_GlossMapScale", "_Glossiness", "_Metallic",
            "_Mode", "_NormalScale", "_NormalScale2", "_Occlusion", "_OcclusionStrength", "_Parallax",
            "_SrcBlend", "_ZWrite",
        };

        private static readonly string[] Textures = {
            "_AlphaMask", "_AnotherRamp", "_ColMask", "_ColorMask", "_DetailMask", "_DisplaceTex",
            "_EmissionMask", "_GlassRamp", "_HairGloss", "_LineMask", "_MainTex", "_NormalMap",
            "_NormalMapDetail", "_NormalMask", "_PatternMask1", "_PatternMask2", "_PatternMask3", "_ReflectMap",
            "_ReflectMapDetail", "_ReflectMask", "_ReflectionMapCap", "_TessTex", "_Texture2", "_Texture3",
            "_Texture4", "_Texture5", "_Texture6", "_Texture7", "_expression", "_liquidmask",
            "_overtex1", "_overtex2", "_overtex3", "_paintmask",
            "_BumpMap", "_EmissionMap", "_MetallicGlossMap", "_NormalMap2", "_OcclusionMap", "_ParallaxMap",
        };

        // Every recorded property the material has, with its stored value.
        private static void Properties(StringBuilder s, Material m)
        {
            s.Append(", \"keywords\": [");
            string[] kw = m.shaderKeywords ?? new string[0];
            for (int i = 0; i < kw.Length; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(Str(kw[i]));
            }
            s.Append("], \"props\": {");
            bool first = true;
            foreach (string n in Colors)
            {
                if (!m.HasProperty(n)) continue;
                s.Append(first ? "" : ", ").Append(Str(n)).Append(": ").Append(C(m.GetColor(n)));
                first = false;
            }
            foreach (string n in Vectors)
            {
                if (!m.HasProperty(n)) continue;
                Vector4 v = m.GetVector(n);
                s.Append(first ? "" : ", ").Append(Str(n)).Append(": [").Append(F(v.x)).Append(", ").Append(F(v.y)).Append(", ").Append(F(v.z)).Append(", ").Append(F(v.w)).Append("]");
                first = false;
            }
            foreach (string n in Floats)
            {
                if (!m.HasProperty(n)) continue;
                s.Append(first ? "" : ", ").Append(Str(n)).Append(": ").Append(F(m.GetFloat(n)));
                first = false;
            }
            s.Append("}, \"textures\": {");
            first = true;
            foreach (string n in Textures)
            {
                if (!m.HasProperty(n)) continue;
                Texture t = m.GetTexture(n);
                Vector2 sc = m.GetTextureScale(n);
                Vector2 of = m.GetTextureOffset(n);
                s.Append(first ? "" : ", ").Append(Str(n)).Append(": {\"name\": ").Append(Str(t != null ? t.name : ""));
                if (t != null)
                {
                    s.Append(", \"size\": [").Append(t.width).Append(", ").Append(t.height).Append("]");
                }
                s.Append(", \"st\": [").Append(F(sc.x)).Append(", ").Append(F(sc.y)).Append(", ").Append(F(of.x)).Append(", ").Append(F(of.y)).Append("]}");
                first = false;
            }
            s.Append("}");
        }

        // Frames between switching IK or FK on and the picture.
        private const int KinematicFrames = 5;

        // Switches on IK or FK as a list line asks. The bones hold the
        // animation's pose here, so the targets and the FK fields start
        // where the animation is, as Studio's copy buttons put them.
        private static void Kinematics(OCIChar chara, Entry e)
        {
            if (e.Ik && e.Fk)
            {
                throw new FormatException("Studio has IK or FK on, not both");
            }
            if (e.Fk)
            {
                chara.fkCtrl.CopyBone();
                for (int i = 0; i < chara.oiCharInfo.activeFK.Length; i++)
                {
                    chara.oiCharInfo.activeFK[i] = true;
                }
                chara.ActiveKinematicMode(OICharInfo.KinematicMode.FK, true, true);
                foreach (KeyValuePair<string, Vector3> turn in e.FkTurns)
                {
                    OCIChar.BoneInfo bone = chara.listBones.FirstOrDefault(b => b.guideObject.transformTarget != null && b.guideObject.transformTarget.name == turn.Key);
                    if (bone == null)
                    {
                        throw new FormatException("no FK bone " + turn.Key);
                    }
                    bone.boneInfo.changeAmount.rot = bone.boneInfo.changeAmount.rot + turn.Value;
                }
            }
            if (e.Ik)
            {
                chara.ikCtrl.CopyBone((OIBoneInfo.BoneGroup)(-1));
                for (int i = 0; i < chara.oiCharInfo.activeIK.Length; i++)
                {
                    chara.oiCharInfo.activeIK[i] = true;
                }
                chara.ActiveKinematicMode(OICharInfo.KinematicMode.IK, true, true);
                foreach (KeyValuePair<int, Vector3[]> move in e.IkMoves)
                {
                    OIIKTargetInfo info;
                    if (!chara.oiCharInfo.ikTarget.TryGetValue(move.Key, out info))
                    {
                        throw new FormatException("no IK target " + move.Key);
                    }
                    OCIChar.IKInfo target = chara.listIKTarget.First(t => t.targetInfo == info);
                    target.targetObject.position += move.Value[0];
                    target.targetObject.rotation = Quaternion.Euler(move.Value[1]) * target.targetObject.rotation;
                    target.guideObject.changeAmount.pos = target.targetObject.localPosition;
                    if (target.guideObject.enableRot)
                    {
                        target.guideObject.changeAmount.rot = target.targetObject.localEulerAngles;
                    }
                }
            }
        }

        // Watches a character's FullBodyBipedIK through one update: every
        // bone as the solver is about to read them with every setting of
        // the solver, its chains, effectors and mappings (private ones too:
        // what it worked out when it started), the solver's points once it
        // has solved, and every bone as it left them.
        private sealed class IkWatch
        {
            private readonly RootMotion.FinalIK.IKSolverFullBodyBiped solver;
            private readonly Transform[] bones;
            private readonly Dictionary<Transform, int> index = new Dictionary<Transform, int>();
            private string read;
            private string settings;
            private string solved;
            private string written;

            public IkWatch(OCIChar chara)
            {
                solver = chara.finalIK.solver;
                bones = chara.charInfo.objBodyBone.GetComponentsInChildren<Transform>(true);
                for (int i = 0; i < bones.Length; i++)
                {
                    index[bones[i]] = i;
                }
                solver.OnPreRead += PreRead;
                solver.OnPostSolve += PostSolve;
                solver.OnPostUpdate += PostUpdate;
            }

            public void Stop()
            {
                solver.OnPreRead -= PreRead;
                solver.OnPostSolve -= PostSolve;
                solver.OnPostUpdate -= PostUpdate;
            }

            private void PreRead()
            {
                if (read == null)
                {
                    read = Bones();
                    settings = Settings();
                }
            }

            private void PostSolve()
            {
                if (read != null && solved == null)
                {
                    solved = Points();
                }
            }

            private void PostUpdate()
            {
                if (read != null && written == null)
                {
                    written = Bones();
                }
            }

            private string Bones()
            {
                StringBuilder s = new StringBuilder("[");
                for (int i = 0; i < bones.Length; i++)
                {
                    Transform t = bones[i];
                    int parent;
                    if (t.parent == null || !index.TryGetValue(t.parent, out parent))
                    {
                        parent = -1;
                    }
                    s.Append(i == 0 ? "\n" : ",\n");
                    s.Append("{\"name\": ").Append(Str(t.name));
                    s.Append(", \"parent\": ").Append(parent);
                    s.Append(", \"t\": ").Append(V(t.localPosition));
                    s.Append(", \"r\": ").Append(Q(t.localRotation));
                    s.Append(", \"s\": ").Append(V(t.localScale));
                    s.Append(", \"pos\": ").Append(V(t.position));
                    s.Append(", \"rot\": ").Append(Q(t.rotation));
                    s.Append("}");
                }
                return s.Append("\n]").ToString();
            }

            private string Points()
            {
                StringBuilder s = new StringBuilder("[");
                RootMotion.FinalIK.IKSolver.Point[] points = solver.GetPoints();
                for (int i = 0; i < points.Length; i++)
                {
                    s.Append(i == 0 ? "\n" : ",\n").Append(Fields(points[i]));
                }
                return s.Append("\n]").ToString();
            }

            private string Settings()
            {
                StringBuilder s = new StringBuilder("{");
                s.Append("\n\"solver\": ").Append(Fields(solver));
                s.Append(",\n\"chains\": [");
                for (int i = 0; i < solver.chain.Length; i++)
                {
                    RootMotion.FinalIK.FBIKChain c = solver.chain[i];
                    s.Append(i == 0 ? "\n" : ",\n");
                    s.Append("{\"chain\": ").Append(Fields(c));
                    s.Append(", \"bend\": ").Append(Fields(c.bendConstraint));
                    s.Append(", \"bend_goal\": ").Append(Place(c.bendConstraint.bendGoal));
                    s.Append(", \"nodes\": [");
                    for (int j = 0; j < c.nodes.Length; j++)
                    {
                        s.Append(j == 0 ? "" : ", ").Append(Fields(c.nodes[j]));
                    }
                    s.Append("]}");
                }
                s.Append("\n],\n\"effectors\": [");
                for (int i = 0; i < solver.effectors.Length; i++)
                {
                    RootMotion.FinalIK.IKEffector f = solver.effectors[i];
                    s.Append(i == 0 ? "\n" : ",\n");
                    s.Append("{\"effector\": ").Append(Fields(f));
                    s.Append(", \"target_place\": ").Append(Place(f.target)).Append("}");
                }
                s.Append("\n],\n\"spine_mapping\": ").Append(Fields(solver.spineMapping));
                s.Append(",\n\"bone_mappings\": [");
                for (int i = 0; i < solver.boneMappings.Length; i++)
                {
                    s.Append(i == 0 ? "\n" : ",\n").Append(Fields(solver.boneMappings[i]));
                }
                s.Append("\n],\n\"limb_mappings\": [");
                for (int i = 0; i < solver.limbMappings.Length; i++)
                {
                    s.Append(i == 0 ? "\n" : ",\n").Append(Fields(solver.limbMappings[i]));
                }
                return s.Append("\n]\n}").ToString();
            }

            // Where a transform is in the world, or null.
            private static string Place(Transform t)
            {
                if (t == null)
                {
                    return "null";
                }
                return "{\"name\": " + Str(t.name) + ", \"pos\": " + V(t.position) + ", \"rot\": " + Q(t.rotation) + "}";
            }

            public string Json()
            {
                StringBuilder s = new StringBuilder("{\n");
                s.Append("\"watched\": ").Append(B(written != null)).Append(",\n");
                s.Append("\"settings\": ").Append(settings ?? "null").Append(",\n");
                s.Append("\"read\": ").Append(read ?? "null").Append(",\n");
                s.Append("\"solved\": ").Append(solved ?? "null").Append(",\n");
                s.Append("\"written\": ").Append(written ?? "null").Append("\n}\n");
                return s.ToString();
            }
        }

        // The fields of an object of the solver, public and private, by
        // name: numbers, switches, enum names, vectors, rotations, and the
        // names of the transforms it refers to; lists of these too.
        private static string Fields(object o)
        {
            if (o == null)
            {
                return "null";
            }
            StringBuilder s = new StringBuilder("{");
            bool first = true;
            for (Type type = o.GetType(); type != null && type != typeof(object); type = type.BaseType)
            {
                foreach (FieldInfo field in type.GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly))
                {
                    string text = Value(field.GetValue(o));
                    if (text == null)
                    {
                        continue;
                    }
                    s.Append(first ? "" : ", ").Append(Str(field.Name)).Append(": ").Append(text);
                    first = false;
                }
            }
            return s.Append("}").ToString();
        }

        private static string Value(object v)
        {
            if (v is float)
            {
                return F((float)v);
            }
            if (v is int)
            {
                return ((int)v).ToString(CultureInfo.InvariantCulture);
            }
            if (v is bool)
            {
                return B((bool)v);
            }
            if (v is Enum)
            {
                return Str(v.ToString());
            }
            if (v is Vector3)
            {
                return V((Vector3)v);
            }
            if (v is Quaternion)
            {
                return Q((Quaternion)v);
            }
            Transform t = v as Transform;
            if (t != null)
            {
                return Str(t.name);
            }
            Array list = v as Array;
            if (list != null)
            {
                Type item = list.GetType().GetElementType();
                if (item != typeof(float) && item != typeof(int) && item != typeof(bool) && item != typeof(Vector3) && item != typeof(Quaternion) && item != typeof(Transform))
                {
                    return null;
                }
                StringBuilder s = new StringBuilder("[");
                for (int i = 0; i < list.Length; i++)
                {
                    s.Append(i == 0 ? "" : ", ").Append(Value(list.GetValue(i)) ?? "null");
                }
                return s.Append("]").ToString();
            }
            return null;
        }

        // Type, state and public fields of one image effect: numbers,
        // switches, enum names, colours, and the names of the textures and
        // shaders it refers to (the colour table of Amplify Color is one).
        private static string EffectFields(Behaviour effect)
        {
            StringBuilder s = new StringBuilder();
            s.Append("{\"type\": ").Append(Str(effect.GetType().FullName));
            s.Append(", \"enabled\": ").Append(B(effect.enabled));
            s.Append(", \"fields\": {");
            bool first = true;
            foreach (FieldInfo field in effect.GetType().GetFields(BindingFlags.Instance | BindingFlags.Public))
            {
                object v = field.GetValue(effect);
                string text = null;
                if (v is float)
                {
                    text = F((float)v);
                }
                else if (v is int)
                {
                    text = ((int)v).ToString(CultureInfo.InvariantCulture);
                }
                else if (v is bool)
                {
                    text = B((bool)v);
                }
                else if (v is Enum)
                {
                    text = Str(v.ToString());
                }
                else if (v is Color)
                {
                    text = C((Color)v);
                }
                else if (typeof(UnityEngine.Object).IsAssignableFrom(field.FieldType))
                {
                    UnityEngine.Object o = v as UnityEngine.Object;
                    text = Str(o != null ? o.name : "");
                }
                if (text == null)
                {
                    continue;
                }
                s.Append(first ? "" : ", ").Append(Str(field.Name)).Append(": ").Append(text);
                first = false;
            }
            s.Append("}}");
            return s.ToString();
        }

        // Unity's own answers for the functions Renkatsu's IK port uses, on
        // fixed pseudo-random inputs and the edge cases, for rk-ik's tests.
        // Each case is one flat row: the inputs, then the result.
        private void UnityMath()
        {
            System.Random rng = new System.Random(20261123);
            Func<float, float, float> num = (lo, hi) => lo + (float)rng.NextDouble() * (hi - lo);
            Func<Vector3> vec = () =>
            {
                Vector3 v = new Vector3(num(-1f, 1f), num(-1f, 1f), num(-1f, 1f));
                return v * Mathf.Pow(10f, num(-2f, 1f));
            };
            Func<Quaternion> rot = () => Quaternion.Euler(num(-180f, 180f), num(-180f, 180f), num(-180f, 180f));
            Vector3[] edges = { Vector3.zero, Vector3.right, Vector3.left, Vector3.up, Vector3.down, Vector3.forward, Vector3.back,
                new Vector3(1e-6f, 0f, 0f), new Vector3(0.3f, -0.4f, 0.8f), -new Vector3(0.3f, -0.4f, 0.8f) };
            StringBuilder s = new StringBuilder("{");
            Action<string, List<float[]>> put = (name, list) =>
            {
                s.Append(s.Length > 1 ? ",\n" : "\n").Append(Str(name)).Append(": [");
                for (int i = 0; i < list.Count; i++)
                {
                    s.Append(i == 0 ? "\n  [" : ",\n  [");
                    for (int j = 0; j < list[i].Length; j++)
                    {
                        s.Append(j == 0 ? "" : ", ").Append(F(list[i][j]));
                    }
                    s.Append("]");
                }
                s.Append("\n]");
            };
            Func<Vector3, float[]> fv = v => new[] { v.x, v.y, v.z };
            Func<Quaternion, float[]> fq = q => new[] { q.x, q.y, q.z, q.w };
            Func<float[][], float[]> row = parts => parts.SelectMany(p => p).ToArray();
            const int N = 400;

            List<float[]> rows = new List<float[]>();
            foreach (Vector3 a in edges)
            {
                foreach (Vector3 b in edges)
                {
                    rows.Add(row(new[] { fv(a), fv(b), fq(Quaternion.LookRotation(a, b)) }));
                }
            }
            for (int i = 0; i < N; i++)
            {
                Vector3 a = vec(), b = vec();
                rows.Add(row(new[] { fv(a), fv(b), fq(Quaternion.LookRotation(a, b)) }));
            }
            put("look_rotation", rows);

            rows = new List<float[]>();
            foreach (Vector3 a in edges)
            {
                foreach (Vector3 b in edges)
                {
                    rows.Add(row(new[] { fv(a), fv(b), fq(Quaternion.FromToRotation(a, b)) }));
                }
            }
            for (int i = 0; i < N; i++)
            {
                Vector3 a = vec();
                Vector3 b = i % 10 == 0 ? -a * num(0.5f, 2f) : vec();
                rows.Add(row(new[] { fv(a), fv(b), fq(Quaternion.FromToRotation(a, b)) }));
            }
            put("from_to_rotation", rows);

            rows = new List<float[]>();
            float[] angles = { 0f, 90f, -90f, 180f, 360f, 720.5f };
            foreach (Vector3 a in edges)
            {
                foreach (float d in angles)
                {
                    rows.Add(row(new[] { new[] { d }, fv(a), fq(Quaternion.AngleAxis(d, a)) }));
                }
            }
            for (int i = 0; i < N; i++)
            {
                float d = num(-400f, 400f);
                Vector3 a = vec();
                rows.Add(row(new[] { new[] { d }, fv(a), fq(Quaternion.AngleAxis(d, a)) }));
            }
            put("angle_axis", rows);

            rows = new List<float[]>();
            for (int i = 0; i < N; i++)
            {
                Quaternion q = rot();
                if (i % 4 == 1)
                {
                    q = new Quaternion(q.x * 1.5f, q.y * 1.5f, q.z * 1.5f, q.w * 1.5f);
                }
                rows.Add(row(new[] { fq(q), fq(Quaternion.Inverse(q)) }));
            }
            put("inverse", rows);

            foreach (string kind in new[] { "lerp", "slerp" })
            {
                rows = new List<float[]>();
                for (int i = 0; i < N; i++)
                {
                    Quaternion a = rot();
                    Quaternion b = i % 5 == 0 ? new Quaternion(-a.x, -a.y, -a.z, -a.w) : i % 5 == 1 ? a * Quaternion.Euler(num(-5f, 5f), num(-5f, 5f), 0f) : rot();
                    float t = i % 7 == 0 ? num(-0.5f, 1.5f) : num(0f, 1f);
                    Quaternion r = kind == "lerp" ? Quaternion.Lerp(a, b, t) : Quaternion.Slerp(a, b, t);
                    rows.Add(row(new[] { fq(a), fq(b), new[] { t }, fq(r) }));
                }
                put("quat_" + kind, rows);
            }

            rows = new List<float[]>();
            foreach (Vector3 a0 in edges)
            {
                foreach (Vector3 b0 in edges)
                {
                    Vector3 a = a0, b = b0;
                    Vector3.OrthoNormalize(ref a, ref b);
                    rows.Add(row(new[] { fv(a0), fv(b0), fv(a), fv(b) }));
                }
            }
            for (int i = 0; i < N; i++)
            {
                Vector3 a0 = vec(), b0 = i % 10 == 0 ? Vector3.zero : vec();
                if (i % 10 == 5)
                {
                    b0 = a0 * num(-2f, 2f);
                }
                Vector3 a = a0, b = b0;
                Vector3.OrthoNormalize(ref a, ref b);
                rows.Add(row(new[] { fv(a0), fv(b0), fv(a), fv(b) }));
            }
            put("ortho_normalize", rows);

            // Managed functions too, as a check on the port of their C#.
            rows = new List<float[]>();
            for (int i = 0; i < N; i++)
            {
                Quaternion a = rot(), b = rot();
                Vector3 v = vec(), w = vec();
                rows.Add(row(new[] { fq(a), fq(b), fv(v), fv(w), fq(a * b), fv(a * v), fv(v.normalized),
                    fv(Vector3.Project(v, w)), new[] { Vector3.Angle(v, w), Vector3.Distance(v, w) } }));
            }
            put("managed", rows);

            s.Append("\n}\n");
            string dir = Path.Combine(Paths.GameRootPath, "UserData/renkatsu/out");
            Directory.CreateDirectory(dir);
            string path = Path.Combine(dir, "unitymath.json");
            File.WriteAllText(path, s.ToString(), new UTF8Encoding(false));
            Logger.LogMessage("Reference capture: Unity maths saved to " + path);
        }

        private static string F(float v)
        {
            if (float.IsNaN(v) || float.IsInfinity(v))
            {
                return "0";
            }
            return v.ToString("R", CultureInfo.InvariantCulture);
        }

        private static string B(bool v)
        {
            return v ? "true" : "false";
        }

        private static string V(Vector3 v)
        {
            return "[" + F(v.x) + ", " + F(v.y) + ", " + F(v.z) + "]";
        }

        private static string Q(Quaternion q)
        {
            return "[" + F(q.x) + ", " + F(q.y) + ", " + F(q.z) + ", " + F(q.w) + "]";
        }

        private static string C(Color c)
        {
            return "[" + F(c.r) + ", " + F(c.g) + ", " + F(c.b) + ", " + F(c.a) + "]";
        }

        // Row by row.
        private static string M(Matrix4x4 m)
        {
            StringBuilder s = new StringBuilder("[");
            for (int i = 0; i < 16; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(F(m[i / 4, i % 4]));
            }
            return s.Append("]").ToString();
        }

        private static string Str(string v)
        {
            StringBuilder s = new StringBuilder("\"");
            foreach (char ch in v)
            {
                if (ch == '"' || ch == '\\')
                {
                    s.Append('\\').Append(ch);
                }
                else if (ch < ' ')
                {
                    s.Append("\\u").Append(((int)ch).ToString("x4"));
                }
                else
                {
                    s.Append(ch);
                }
            }
            return s.Append('"').ToString();
        }
    }
}
