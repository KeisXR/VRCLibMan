# Importing a VRCLibMan package into Unity

`product_files` gives an absolute `path` to either a `.unitypackage` or a `.zip`. VRCLibMan only reads; getting the package into the project is your step, and only after the user agrees.

## Package is a plain .unitypackage

Use the Unity MCP server that is connected (for example MCP for Unity). Run editor code equivalent to:

```csharp
UnityEditor.AssetDatabase.ImportPackage(@"<path>", true); // true = show Unity's import dialog
```

Prefer the interactive dialog (`true`) so the user can review what is added. If the Unity MCP server has no way to run editor code, tell the user the path and ask them to drag it into the Project window, or use 「Unityで開く」 in VRCLibMan.

## Package is inside a .zip

Each zip file in `product_files` lists `unityPackages` with an `entry` (path inside the zip). Extract only that entry to a temporary folder outside the Unity project and the library, then import the extracted file as above. Do not extract the whole zip into the project, and do not modify or delete the original zip.

## Before importing

- Confirm which avatar the outfit/hair is for, and which variant if the zip holds several.
- Many packages expect a shader (e.g. lilToon, Poiyomi) or Modular Avatar to be installed first. If the product description snippet or title mentions MA対応, lilToon, etc., check the project has them and tell the user if not.
- Import into the project the user is working in; never import into a different project.

## After importing

Report what was imported. Placing the outfit on the avatar (Modular Avatar setup, fitting tools such as もちふぃった～) is a separate step; offer it rather than doing it automatically.
