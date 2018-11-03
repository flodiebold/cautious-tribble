import * as React from "react";
// @ts-ignore
import { useState } from "react";

import Button from "@material-ui/core/Button";
import Checkbox from "@material-ui/core/Checkbox";
import CircularProgress from "@material-ui/core/CircularProgress";
import Dialog from "@material-ui/core/Dialog";
import DialogActions from "@material-ui/core/DialogActions";
import DialogContent from "@material-ui/core/DialogContent";
import DialogContentText from "@material-ui/core/DialogContentText";
import DialogTitle from "@material-ui/core/DialogTitle";
import Divider from "@material-ui/core/Divider";
import FormControl from "@material-ui/core/FormControl";
import FormControlLabel from "@material-ui/core/FormControlLabel";
import FormGroup from "@material-ui/core/FormGroup";
import FormHelperText from "@material-ui/core/FormHelperText";
import FormLabel from "@material-ui/core/FormLabel";
import TextField from "@material-ui/core/TextField";
import withMobileDialog from "@material-ui/core/withMobileDialog";

import { IResourceVersion } from ".";
import { deploy } from "./api";

export interface IVersionDialogProps {
    onClose: () => void;
    resource: string;
    deployableEnvs: string[];
    version: IResourceVersion;
}

export function VersionDialog(props: IVersionDialogProps) {
    const { onClose, version, resource, deployableEnvs } = props;
    const [deployEnvs, setDeployEnvs] = useState({});
    const [reasonMessage, setReasonMessage] = useState("");
    const [deploying, setDeploying] = useState(false);
    const deployEnabled =
        Object.keys(deployEnvs).some(k => !!deployEnvs[k]) &&
        reasonMessage !== "";

    const handleDeploy = async () => {
        setDeploying(true);
        const deployEnvNames = Object.keys(deployEnvs).filter(
            env => deployEnvs[env]
        );
        const joinedEnvNames = deployEnvNames.join(",");
        const data = {
            message: `Deploying ${resource} to ${
                version.version
            } on ${joinedEnvNames} via UI\n\n${reasonMessage}`,
            deployments: deployEnvNames.map(env => ({
                resource,
                version_id: version.version_id,
                env
            }))
        };
        try {
            await deploy(data);

            props.onClose();
        } catch (e) {
            // TODO handle error
            console.error("error deploying", e); // tslint:disable-line
        } finally {
            setDeploying(false);
        }
    };

    return (
        <Dialog
            open
            onClose={onClose}
            aria-labelledby="responsive-dialog-title"
        >
            <DialogTitle id="responsive-dialog-title">
                {resource} {version.version}
            </DialogTitle>
            <DialogContent>
                <DialogContentText style={{ whiteSpace: "pre" }}>
                    {version.change_log}
                </DialogContentText>
            </DialogContent>
            <Divider />
            <DialogContent>
                <FormControl component="fieldset" margin="normal">
                    <FormLabel component="legend">Deploy to</FormLabel>
                    <FormGroup>
                        {deployableEnvs.map(env => (
                            <FormControlLabel
                                key={env}
                                control={
                                    <Checkbox
                                        checked={!!deployEnvs[env]}
                                        onChange={e =>
                                            setDeployEnvs(
                                                Object.assign({}, deployEnvs, {
                                                    [env]: e.target.checked
                                                })
                                            )
                                        }
                                        value={env}
                                    />
                                }
                                label={env}
                            />
                        ))}
                    </FormGroup>
                    <TextField
                        id="reason-message"
                        label="Reason message"
                        multiline
                        rows="4"
                        value={reasonMessage}
                        onChange={e => setReasonMessage(e.target.value)}
                        margin="none"
                        variant="filled"
                    />
                </FormControl>
            </DialogContent>
            <DialogActions disableActionSpacing>
                <Button onClick={onClose} color="primary" autoFocus>
                    Cancel
                </Button>
                <div style={{ position: "relative" }}>
                    <Button
                        variant="contained"
                        color="primary"
                        disabled={!deployEnabled || deploying}
                        onClick={handleDeploy}
                    >
                        Deploy
                    </Button>
                    {deploying && (
                        <CircularProgress
                            size={24}
                            style={{
                                position: "absolute",
                                top: "50%",
                                left: "50%",
                                marginTop: -12,
                                marginLeft: -12
                            }}
                        />
                    )}
                </div>
            </DialogActions>
        </Dialog>
    );
}
