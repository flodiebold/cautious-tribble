import * as React from "react";

import Grid from "@material-ui/core/Grid";
import Paper from "@material-ui/core/Paper";
import Table from "@material-ui/core/Table";
import TableBody from "@material-ui/core/TableBody";
import TableCell from "@material-ui/core/TableCell";
import TableHead from "@material-ui/core/TableHead";
import TableRow from "@material-ui/core/TableRow";

import {
    IResourceRepoCommit,
    IUiData,
    IVersionDeployed,
    ResourceRepoChange
} from "./index";

interface IHistoryViewProps {
    data: IUiData;
}

function getGroup(change: ResourceRepoChange): [string, string] | null {
    switch (change.change) {
        case "VersionDeployed":
            if (change.env === "latest") {
                return null; // FIXME
            }
            if (change.previous_version_id === null) {
                return [`Newly deployed to ${change.env}:`, change.resource];
            } else {
                return [`Updated on ${change.env}:`, change.resource];
            }

        case "Version":
            return [`New version for ${change.resource}:`, change.version];

        case "BaseData":
        case "Deployable":
        default:
            return null;
    }
}

function CommitRow({ commit }: { commit: IResourceRepoCommit }) {
    const time = new Date(commit.time);
    const groupedChanges = commit.changes.reduce((groups, c) => {
        const groupResult = getGroup(c);
        if (!groupResult) {
            return groups;
        }
        const [group, value] = groupResult;
        if (!groups.has(group)) {
            groups.set(group, []);
        }
        groups.get(group).push(value);
        return groups;
    }, new Map());
    const newlyDeployed = commit.changes
        .filter(
            c =>
                c.change === "VersionDeployed" && c.previous_version_id === null
        )
        .map(c => (c as IVersionDeployed).resource);
    return (
        <TableRow>
            <TableCell>{time.toLocaleString()}</TableCell>
            <TableCell style={{ whiteSpace: "pre-line" }}>
                {commit.message}
            </TableCell>
            <TableCell>
                {[...groupedChanges.entries()].map(([typ, group]) => (
                    <p key={typ}>
                        {typ} {group.join(", ")}
                    </p>
                ))}
            </TableCell>
        </TableRow>
    );
}

export function HistoryView(props: IHistoryViewProps) {
    const history = props.data.history;
    const reversed = props.data.history.slice().reverse();
    return (
        <Grid container spacing={1} style={{ padding: 16 }}>
            <Grid item xs={12}>
                <Paper>
                    <Table>
                        <TableBody>
                            {reversed.map(commit => (
                                <CommitRow key={commit.id} commit={commit} />
                            ))}
                        </TableBody>
                    </Table>
                </Paper>
            </Grid>
        </Grid>
    );
}
