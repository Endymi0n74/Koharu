#!/usr/bin/env bun
// Bannit les littéraux de chemin Windows non portables dans le code Rust.
//
// Motivation : le test folder_project_name utilisait Path::new(r"C:\scans\a:b*c").
// Sur Windows file_name() = "a:b*c" ; sur Linux le backslash n'y est pas un
// séparateur, donc file_name() renvoie toute la chaîne → la suite Test cassait
// sur ubuntu-latest (fix 3cd290b9). Un littéral Windows ne devrait jamais
// entrer dans un test exécuté sur les deux plateformes.
//
// Règles (scan ligne par ligne, sources crates/**/*.rs) :
//   1. Path::new/PathBuf::from/.join avec un raw string contenant un backslash
//   2. idem avec une chaîne normale contenant \\ (backslash échappé)
//   3. lettre de lecteur suivie d'un backslash dans n'importe quel littéral
//      ("C:\..." ou "D:\\...") — y compris hors constructeur de chemin
//
// Volontairement NON interdit : lecteur + slash avant ("Z:/..." portable au
// niveau de file_name), et les fixtures TS affichant un message d'erreur
// Windows (données d'affichage, hors périmètre Rust).
//
//   bun scripts/check-path-portability.ts [fichiers-ou-dossiers...]
//                                         (défaut : crates)

import { readdirSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'

interface Rule {
  name: string
  pattern: RegExp
}

interface Violation {
  file: string
  line: number
  rule: string
  text: string
}

const RULES: Rule[] = [
  {
    name: 'chemin Windows en raw string (backslash non portable)',
    pattern: /(?:Path::new|PathBuf::from|\.join)\(\s*r#*"[^"\n]*\\/,
  },
  {
    name: 'chemin Windows en chaîne échappée ("\\\\")',
    pattern: /(?:Path::new|PathBuf::from|\.join)\(\s*"[^"\n]*\\\\/,
  },
  {
    name: 'lettre de lecteur + backslash dans un littéral',
    // Le drive letter est seul (pas précédé d'une lettre) : sans ça,
    // "logs:\n" ou "Assistant:\n" matchent sur le :\ de l'échappement.
    pattern: /(?<![A-Za-z])[A-Za-z]:\\/,
  },
]

const SKIP_DIRS = new Set(['target', 'node_modules', '.git'])

function rustFiles(roots: string[]): string[] {
  const files: string[] = []
  const visit = (entry: string) => {
    const stat = statSync(entry)
    if (stat.isDirectory()) {
      if (SKIP_DIRS.has(path.basename(entry))) return
      for (const child of readdirSync(entry)) visit(path.join(entry, child))
    } else if (entry.endsWith('.rs')) {
      files.push(entry)
    }
  }
  for (const root of roots) visit(root)
  return files.sort()
}

function scan(files: string[]): Violation[] {
  const violations: Violation[] = []
  for (const file of files) {
    const lines = readFileSync(file, 'utf8').split('\n')
    lines.forEach((text, index) => {
      for (const rule of RULES) {
        if (rule.pattern.test(text)) {
          violations.push({ file, line: index + 1, rule: rule.name, text: text.trim() })
          break
        }
      }
    })
  }
  return violations
}

function main(): number {
  const roots = process.argv.slice(2)
  const files = rustFiles(roots.length > 0 ? roots : ['crates'])
  const violations = scan(files)
  if (violations.length === 0) {
    console.log(`check-path-portability: ${files.length} fichiers Rust vérifiés, aucun littéral Windows.`)
    return 0
  }
  console.error(`check-path-portability: ${violations.length} littéral(aux) Windows non portables :\n`)
  for (const violation of violations) {
    console.error(`${violation.file}:${violation.line}: ${violation.rule}`)
    console.error(`    ${violation.text}`)
  }
  console.error('\nUtilisez des slashs avant ("/scans/chapitre 01") : file_name() doit être identique sous Windows et Linux.')
  return 1
}

process.exit(main())
