import { existsSync, readFileSync } from 'fs';
import path from 'path';
import { createRequire } from 'module';
import { fileURLToPath } from 'url';
import { defineConfig, loadEnv } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

type PackageManifest = {
  name: string;
  version: string;
  dependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
};

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const semiUiDir = path.resolve(
  path.dirname(require.resolve('@douyinfe/semi-ui')),
  '../..',
);
// Hoisted workspace packages otherwise bind Classic consumers to Default's newer majors.
const semiDateFnsDir = path.dirname(
  require.resolve('date-fns/package.json', { paths: [semiUiDir] }),
);

const readPackageManifest = (packageDir: string) =>
  JSON.parse(
    readFileSync(path.join(packageDir, 'package.json'), 'utf8'),
  ) as PackageManifest;

const resolvePackageDir = (packageName: string, ownerDir: string) => {
  let packageDir = path.dirname(
    require.resolve(packageName, { paths: [ownerDir] }),
  );

  while (true) {
    const manifestPath = path.join(packageDir, 'package.json');
    if (existsSync(manifestPath)) {
      const manifest = readPackageManifest(packageDir);
      if (manifest.name === packageName) {
        return packageDir;
      }
    }

    const parentDir = path.dirname(packageDir);
    if (parentDir === packageDir) {
      throw new Error(`Unable to locate package root for ${packageName}`);
    }
    packageDir = parentDir;
  }
};

// VRender keeps browser environment state in module singletons, so alias the graph together.
const collectClassicVisActorAliases = () => {
  const pending = [
    '@visactor/react-vchart',
    '@visactor/vchart',
    '@visactor/vchart-semi-theme',
  ].map((name) => ({ name, ownerDir: __dirname }));
  const packages = new Map<string, { packageDir: string; version: string }>();

  while (pending.length > 0) {
    const current = pending.shift();
    if (!current) {
      continue;
    }

    const packageDir = resolvePackageDir(current.name, current.ownerDir);
    const manifest = readPackageManifest(packageDir);
    const existing = packages.get(current.name);

    if (existing) {
      if (existing.version !== manifest.version) {
        throw new Error(
          `Classic VisActor dependency conflict for ${current.name}: ` +
            `${existing.version} at ${existing.packageDir}, ` +
            `${manifest.version} at ${packageDir}`,
        );
      }
      continue;
    }

    packages.set(current.name, { packageDir, version: manifest.version });

    const dependencies = {
      ...manifest.dependencies,
      ...manifest.optionalDependencies,
      ...manifest.peerDependencies,
    };
    for (const dependencyName of Object.keys(dependencies)) {
      if (dependencyName.startsWith('@visactor/')) {
        pending.push({ name: dependencyName, ownerDir: packageDir });
      }
    }
  }

  return Object.fromEntries(
    [...packages].map(([packageName, { packageDir }]) => [
      packageName,
      packageDir,
    ]),
  );
};

const classicVisActorAliases = collectClassicVisActorAliases();

export default defineConfig(({ envMode }) => {
  const env = loadEnv({ mode: envMode, prefixes: ['VITE_'] });
  const clientServerUrl =
    process.env.VITE_REACT_APP_SERVER_URL ||
    env.rawPublicVars.VITE_REACT_APP_SERVER_URL ||
    '';
  const proxyServerUrl = clientServerUrl || 'http://localhost:3000';
  const isProd = envMode === 'production';
  const devProxy = Object.fromEntries(
    (['/api', '/mj', '/pg'] as const).map((key) => [
      key,
      { target: proxyServerUrl, changeOrigin: true },
    ]),
  ) as Record<string, { target: string; changeOrigin: boolean }>;

  return {
    plugins: [pluginReact()],
    source: {
      entry: {
        index: './src/index.jsx',
      },
      define: {
        'import.meta.env.VITE_REACT_APP_SERVER_URL':
          JSON.stringify(clientServerUrl),
      },
    },
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
        'date-fns': semiDateFnsDir,
        ...classicVisActorAliases,
        '@douyinfe/semi-ui/dist/css/semi.css': path.resolve(
          semiUiDir,
          'dist/css/semi.css',
        ),
      },
    },
    html: {
      template: './index.html',
    },
    server: {
      host: '0.0.0.0',
      strictPort: false,
      proxy: devProxy,
    },
    output: {
      minify: isProd,
      target: 'web',
      distPath: {
        root: 'dist',
      },
    },
    performance: {
      removeConsole: isProd ? ['log'] : false,
      buildCache: {
        cacheDigest: [process.env.VITE_REACT_APP_VERSION],
      },
    },
    tools: {
      rspack: {
        module: {
          rules: [
            {
              test: /src[\\/].*\.js$/,
              type: 'javascript/auto',
              use: [
                {
                  loader: 'builtin:swc-loader',
                  options: {
                    jsc: {
                      parser: {
                        syntax: 'ecmascript',
                        jsx: true,
                      },
                      transform: {
                        react: {
                          runtime: 'automatic',
                          development: !isProd,
                          refresh: !isProd,
                        },
                      },
                    },
                  },
                },
              ],
            },
          ],
        },
      },
    },
  };
});
