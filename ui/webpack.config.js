const path = require("path");
const process = require("process");
const convert = require("koa-connect");
const proxy = require("http-proxy-middleware");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const HtmlWebpackIncludeAssetsPlugin = require("html-webpack-include-assets-plugin");

module.exports = {
    entry: "./src/index.tsx",

    plugins: [
        new CopyWebpackPlugin([
            {
                from: "./node_modules/react/umd/react.development.js",
                to: "react.development.js"
            },
            {
                from: "./node_modules/react-dom/umd/react-dom.development.js",
                to: "react-dom.development.js"
            }
        ]),
        new HtmlWebpackPlugin({
            hash: true,
            template: "./src/index.html",
            filename: "index.html"
        }),
        new HtmlWebpackIncludeAssetsPlugin({
            assets: ["react.development.js", "react-dom.development.js"],
            append: false,
            hash: true
        })
    ],

    mode: process.env.WEBPACK_SERVE ? "development" : "production",

    output: {
        filename: "./bundle.js"
    },

    // Enable sourcemaps for debugging webpack's output.
    devtool: "source-map",

    resolve: {
        // Add '.ts' and '.tsx' as resolvable extensions.
        extensions: [".ts", ".tsx", ".js", ".json"]
    },

    module: {
        rules: [
            // All files with a '.ts' or '.tsx' extension will be handled by 'awesome-typescript-loader'.
            { test: /\.tsx?$/, loader: "awesome-typescript-loader" },

            // All output '.js' files will have any sourcemaps re-processed by 'source-map-loader'.
            { enforce: "pre", test: /\.js$/, loader: "source-map-loader" }
        ]
    },

    // When importing a module whose path matches one of the following, just
    // assume a corresponding global variable exists and use that instead.
    // This is important because it allows us to avoid bundling all of our
    // dependencies, which allows browsers to cache those libraries between builds.
    externals: {
        react: "React",
        "react-dom": "ReactDOM"
    },

    serve: {
        content: [__dirname],
        add: (app, middleware, options) => {
            app.use(
                convert(
                    proxy("/api", { target: "http://localhost:9003", ws: true })
                )
            );
            // app.use(convert(history()));
        }
    }
};
