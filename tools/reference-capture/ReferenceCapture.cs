// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later
//
// BepInEx plugin for CharaStudio. On a hotkey it loads each card from a list,
// saves two pictures of it and a JSON file describing the camera, lights,
// global shader values, pose and drawn materials. Renkatsu uses these as the
// reference when checking its own renders.
//
// Input:  UserData/renkatsu/cards.txt, one card path per line (relative paths
//         start at UserData/chara/female). Without the file, the default card.
// Output: UserData/renkatsu/out/<name>.<view>.studio.png  with Studio's effects
//         UserData/renkatsu/out/<name>.<view>.plain.png   image effects off
//         UserData/renkatsu/out/<name>.json
//         The views are "body" and "face".

using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text;
using BepInEx;
using BepInEx.Configuration;
using Studio;
using UnityEngine;

namespace Renkatsu
{
    [BepInPlugin("renkatsu.referencecapture", "Renkatsu reference capture", "0.3.0")]
    [BepInProcess("CharaStudio")]
    public class ReferenceCapture : BaseUnityPlugin
    {
        private const int Width = 900;
        private const int Height = 1400;
        private const int SettleFrames = 90;

        private ConfigEntry<KeyboardShortcut> hotkey;
        private bool running;

        private void Awake()
        {
            hotkey = Config.Bind("General", "Capture hotkey", new KeyboardShortcut(KeyCode.F9, KeyCode.LeftControl, KeyCode.LeftShift),
                "Loads every card from UserData/renkatsu/cards.txt and saves reference pictures.");
        }

        private void Update()
        {
            if (!running && hotkey.Value.IsDown() && Singleton<Studio.Studio>.IsInstance())
            {
                StartCoroutine(Run());
            }
        }

        private IEnumerator Run()
        {
            running = true;
            string root = Path.Combine(Paths.GameRootPath, "UserData/renkatsu");
            string outDir = Path.Combine(root, "out");
            Directory.CreateDirectory(outDir);
            List<string> cards = ReadList(Path.Combine(root, "cards.txt"));
            Logger.LogMessage("Reference capture: " + cards.Count + " cards");

            int done = 0;
            foreach (string card in cards)
            {
                if (!File.Exists(card))
                {
                    Logger.LogWarning("Reference capture: missing " + card);
                    continue;
                }
                OCICharFemale chara = null;
                try
                {
                    chara = AddObjectFemale.Add(card);
                }
                catch (Exception e)
                {
                    Logger.LogError("Reference capture: could not load " + card + ": " + e);
                }
                if (chara == null)
                {
                    continue;
                }
                // A blink in the middle of a picture would make runs differ.
                chara.charInfo.ChangeEyesBlinkFlag(false);
                for (int i = 0; i < SettleFrames; i++)
                {
                    yield return null;
                }
                yield return new WaitForEndOfFrame();
                try
                {
                    Capture(chara, card, Path.Combine(outDir, Path.GetFileNameWithoutExtension(card)));
                    done++;
                }
                catch (Exception e)
                {
                    Logger.LogError("Reference capture: failed on " + card + ": " + e);
                }
                Singleton<Studio.Studio>.Instance.treeNodeCtrl.DeleteNode(chara.treeNodeObject);
                for (int i = 0; i < 5; i++)
                {
                    yield return null;
                }
            }
            Logger.LogMessage("Reference capture: saved " + done + " of " + cards.Count + " to " + outDir);
            running = false;
        }

        private static List<string> ReadList(string listPath)
        {
            string female = Path.Combine(Paths.GameRootPath, "UserData/chara/female");
            List<string> cards = new List<string>();
            if (File.Exists(listPath))
            {
                foreach (string raw in File.ReadAllLines(listPath))
                {
                    string line = raw.Trim();
                    if (line.Length == 0 || line.StartsWith("#"))
                    {
                        continue;
                    }
                    cards.Add(Path.IsPathRooted(line) ? line : Path.Combine(female, line));
                }
            }
            if (cards.Count == 0)
            {
                cards.Add(Path.Combine(female, "Simple.png"));
            }
            return cards;
        }

        private struct View
        {
            public string name;
            public Vector3 position;
            public Quaternion rotation;
        }

        private void Capture(OCICharFemale chara, string card, string outBase)
        {
            Camera cam = Camera.main;
            ChaControl cha = chara.charInfo;
            Renderer[] renderers = cha.GetComponentsInChildren<Renderer>(false);

            // Remember what gets changed, so Studio is left as it was.
            Vector3 oldPos = cam.transform.position;
            Quaternion oldRot = cam.transform.rotation;
            CameraClearFlags oldFlags = cam.clearFlags;
            Color oldBack = cam.backgroundColor;

            float aspect = (float)Width / Height;
            Vector3 forward = cha.transform.forward;
            List<View> views = new List<View>();
            views.Add(Frame("body", ExactBounds(renderers), 1.05f, cam.fieldOfView, aspect, forward));
            if (cha.objHead != null)
            {
                // Wide enough to keep most of the hair in the picture.
                views.Add(Frame("face", ExactBounds(cha.objHead.GetComponentsInChildren<Renderer>(false)), 1.7f, cam.fieldOfView, aspect, forward));
            }

            cam.clearFlags = CameraClearFlags.SolidColor;
            cam.backgroundColor = new Color(0.93f, 0.93f, 0.95f, 1f);
            List<Behaviour> effects = ImageEffects(cam);
            try
            {
                File.WriteAllText(outBase + ".json", Describe(cam, cha, chara, card, renderers, effects, aspect, views), new UTF8Encoding(false));
                foreach (View v in views)
                {
                    cam.transform.position = v.position;
                    cam.transform.rotation = v.rotation;
                    File.WriteAllBytes(outBase + "." + v.name + ".studio.png", Shot(cam));
                    foreach (Behaviour b in effects)
                    {
                        b.enabled = false;
                    }
                    File.WriteAllBytes(outBase + "." + v.name + ".plain.png", Shot(cam));
                    foreach (Behaviour b in effects)
                    {
                        b.enabled = true;
                    }
                }
            }
            finally
            {
                foreach (Behaviour b in effects)
                {
                    b.enabled = true;
                }
                cam.transform.position = oldPos;
                cam.transform.rotation = oldRot;
                cam.clearFlags = oldFlags;
                cam.backgroundColor = oldBack;
            }
        }

        // A camera in front of the character that fits the box, with a margin.
        private static View Frame(string name, Bounds box, float margin, float fov, float aspect, Vector3 forward)
        {
            float half = Mathf.Max(box.extents.y, box.extents.x / aspect) * margin;
            float dist = half / Mathf.Tan(fov * 0.5f * Mathf.Deg2Rad) + box.extents.z;
            View v = new View();
            v.name = name;
            v.position = box.center + forward * dist;
            v.rotation = Quaternion.LookRotation(-forward, Vector3.up);
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

        // Enabled components on the camera that post-process the picture.
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
                    if (t.GetMethod("OnRenderImage", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly) != null)
                    {
                        found.Add(b);
                        break;
                    }
                }
            }
            return found;
        }

        private static byte[] Shot(Camera cam)
        {
            RenderTexture rt = new RenderTexture(Width, Height, 24, RenderTextureFormat.ARGB32);
            rt.antiAliasing = 8;
            RenderTexture oldTarget = cam.targetTexture;
            RenderTexture oldActive = RenderTexture.active;
            Texture2D tex = new Texture2D(Width, Height, TextureFormat.RGB24, false);
            try
            {
                cam.targetTexture = rt;
                cam.Render();
                RenderTexture.active = rt;
                tex.ReadPixels(new Rect(0, 0, Width, Height), 0, 0);
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

        private static string Describe(Camera cam, ChaControl cha, OCICharFemale chara, string card, Renderer[] renderers, List<Behaviour> effects, float aspect, List<View> views)
        {
            SceneInfo scene = Singleton<Studio.Studio>.Instance.sceneInfo;
            StringBuilder s = new StringBuilder(1 << 20);
            s.Append("{\n");
            s.Append("\"card\": ").Append(Str(card)).Append(",\n");
            s.Append("\"unity\": ").Append(Str(Application.unityVersion)).Append(",\n");
            s.Append("\"color_space\": ").Append(Str(QualitySettings.activeColorSpace.ToString())).Append(",\n");
            s.Append("\"size\": [").Append(Width).Append(", ").Append(Height).Append("],\n");

            s.Append("\"camera\": {");
            s.Append("\"fov\": ").Append(F(cam.fieldOfView));
            s.Append(", \"aspect\": ").Append(F(aspect));
            s.Append(", \"near\": ").Append(F(cam.nearClipPlane));
            s.Append(", \"far\": ").Append(F(cam.farClipPlane));
            s.Append(", \"hdr\": ").Append(B(cam.allowHDR));
            s.Append(", \"path\": ").Append(Str(cam.actualRenderingPath.ToString()));
            s.Append(", \"background\": ").Append(C(cam.backgroundColor));
            s.Append("},\n");

            // One picture pair per view: <name>.<view>.studio.png and .plain.png.
            s.Append("\"views\": [");
            for (int i = 0; i < views.Count; i++)
            {
                s.Append(i == 0 ? "" : ", ");
                s.Append("{\"name\": ").Append(Str(views[i].name));
                s.Append(", \"position\": ").Append(V(views[i].position));
                s.Append(", \"rotation\": ").Append(Q(views[i].rotation));
                s.Append("}");
            }
            s.Append("],\n");

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

            s.Append("\"effects_on_camera\": [");
            for (int i = 0; i < effects.Count; i++)
            {
                s.Append(i == 0 ? "" : ", ").Append(Str(effects[i].GetType().FullName));
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
            s.Append(", \"anime_speed\": ").Append(F(chara.animeSpeed));
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
            s.Append("},\n");

            s.Append("\"lights\": [");
            bool first = true;
            foreach (Light l in FindObjectsOfType<Light>())
            {
                if (!l.isActiveAndEnabled)
                {
                    continue;
                }
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
                s.Append("}");
            }
            s.Append("\n],\n");

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
                SkinnedMeshRenderer skinned = r as SkinnedMeshRenderer;
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
                    s.Append("}");
                }
                s.Append("]}");
            }
            s.Append("\n]\n}\n");
            return s.ToString();
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
